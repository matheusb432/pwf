//! `pwf session` — spawn a real agent as a new tab in the item's per-project
//! zellij session. Dispatch drives the external tools through the
//! `MultiplexerDriver`, `InlineExec`, and `AgentLauncher` seams; the pure pieces
//! (outcome rendering, confirmation text, session/tab naming) are unit-tested,
//! and the end-to-end wiring is covered by the `session_*` binary tests against
//! a real zellij stub.

mod confirmation;
mod inline;
mod launcher;
mod model_tiers;
mod multiplexer;
mod render;

pub(in crate::engines::pending_work) use inline::{InlineExec, RealExec};
pub(in crate::engines::pending_work) use launcher::{AgentLauncher, launcher_for};
// The concrete launchers are referenced by name only in `verify`'s tests;
// production selects via `launcher_for`, holding `&'static dyn AgentLauncher`.
#[cfg(test)]
pub(in crate::engines::pending_work) use launcher::{ClaudeLauncher, CodexLauncher};
pub(in crate::engines::pending_work) use model_tiers::{
    ModelTiersError, resolve_model, resolve_model_for_verify,
};
pub(in crate::engines::pending_work) use multiplexer::{
    MultiplexerDriver, NewTabError, RealZellij,
};
use pwf_application::{AppDbStore, PendingWorkItem};
use pwf_domain::pending_work::ProjectRegistry;
pub(super) use render::{DispatchOutcome, render};

use super::color::use_color;
use crate::{
    cli::ColorChoice,
    confirm::{Confirm, DefaultAnswer, RealConfirm},
    engines::pending_work::{
        agent::probe::{AgentProbe, RealProbe},
        errors::PendingWorkError,
        launch::LaunchPolicy,
        model::Item,
        query::find_pending_item,
    },
};

/// Dispatch policy flags, bundled so `dispatch` takes one named value instead
/// of loose scalars (no adjacent-bool transposition, and under clippy's
/// argument-count threshold). Not `Copy` — `model_override` is owned —
/// so call sites that need it after passing it on (e.g. `confirmation::question`)
/// take it by reference.
#[derive(Debug, Clone)]
pub(in crate::engines::pending_work) struct DispatchOpts {
    pub color: ColorChoice,
    pub assume_yes: bool,
    pub inline: bool,
    /// Which optional instruction blocks ride in the launch prompt (`-w`, `--auto`).
    pub launch: LaunchPolicy,
    /// Which agent to dispatch (`-a`/`--agent`).
    pub agent: crate::cli::Agent,
    /// Explicit `--model` override; wins over effort-tier resolution when set.
    pub model_override: Option<String>,
}

/// Real-world entrypoint for `pwf session <id>`: resolves the live claude probe
/// and zellij/agent drivers, warns once if claude is absent from this PATH, then
/// dispatches. Keeps `run.rs` a thin match arm — the wiring lives here, beside
/// the orchestration it drives.
pub(in crate::engines::pending_work) fn dispatch(
    store: &impl AppDbStore<PendingWorkItem>,
    projects: &ProjectRegistry,
    id: &str,
    opts: &DispatchOpts,
) -> Result<String, PendingWorkError> {
    let launcher = launcher_for(opts.agent);
    if !RealProbe::resolve(launcher.binary()).available() {
        eprintln!(
            "note: {} not found on PATH from here; the agent will surface the error if it can't run.",
            launcher.binary()
        );
    }
    {
        let driver = &RealZellij;
        let exec = &RealExec;
        let confirmer = &RealConfirm;
        let item = find_pending_item(store, projects, id)?;
        if !item.launchable {
            return Err(PendingWorkError::NotLaunchable {
                id: item.id,
                issues: item.issues,
            });
        }
        let model = resolve_model(opts.agent, &item, opts.model_override.as_deref())?;
        let repo = item.repo.clone().unwrap_or_default();
        if !std::path::Path::new(&repo).is_dir() {
            return Err(PendingWorkError::RepoMissing {
                project: item.project.clone(),
                path: repo,
            });
        }
        if !opts.inline && !driver.available() {
            return Err(PendingWorkError::ZellijNotFound);
        }
        let session = session_name_for(&item);
        let argv = launcher.argv(&item, opts.launch, model.as_deref());
        if !opts.assume_yes && confirmer.interactive() {
            let question = confirmation::question(&item, &session, opts, launcher.binary());
            if !confirmer.confirm(&question, DefaultAnswer::Yes) {
                return Ok(format!(
                    "# session {} — aborted\nnothing dispatched.\n",
                    item.id
                ));
            }
        }
        if opts.inline {
            // Breadcrumb to stderr: on Unix this is pwf's last word before exec
            // replaces it; on the spawn-and-wait fallback it precedes the child.
            eprintln!("running {} inline in {repo}…", item.id);
            exec.run(&argv, &repo)
                .map_err(|message| PendingWorkError::InlineExecFailed { message })?;
            // Reached only on the non-Unix success path (Unix exec never returns).
            return Ok(format!("# session {} — ran inline\n", item.id));
        }
        let tab = tab_name(&item);
        let outcome = match driver.new_tab(&session, &repo, &tab, &argv) {
            Ok(()) => DispatchOutcome::Success { session, tab },
            Err(NewTabError::SessionNotFound) => {
                driver.ensure_session(&session).map_err(|message| {
                    PendingWorkError::SessionDispatchFailed {
                        session: session.clone(),
                        message,
                    }
                })?;
                match driver.new_tab(&session, &repo, &tab, &argv) {
                    Ok(()) => DispatchOutcome::Fallback { session, tab },
                    Err(e) => DispatchOutcome::Error {
                        session,
                        tab,
                        message: e.to_string(),
                    },
                }
            }
            Err(NewTabError::Other(message)) => DispatchOutcome::Error {
                session,
                tab,
                message,
            },
        };
        Ok(render(
            &outcome,
            launcher.binary(),
            &repo,
            use_color(opts.color),
        ))
    }
}

/// Zellij session name for an item: the lowercased id prefix (`CFG-0009` → `cfg`),
/// which equals the per-project session name in the managed zellij stack.
fn session_name_for(item: &Item) -> String {
    item.id
        .split('-')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

/// The new tab's name: the canonical item id. Agent-independent — every harness
/// runs under a tab named for the item it dispatches, so this is not part of the
/// `AgentLauncher` seam.
fn tab_name(item: &Item) -> String {
    item.id.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::pending_work::model::Item;

    #[test]
    fn session_name_is_the_lowercased_id_prefix() {
        let item = Item::default_for_test("CFG-0009", "x");
        assert_eq!(session_name_for(&item), "cfg");
    }

    #[test]
    fn session_name_takes_only_the_first_hyphen_segment() {
        // The prefix, not the whole id, names the per-project zellij session.
        let item = Item::default_for_test("PWF-0001", "x");
        assert_eq!(session_name_for(&item), "pwf");
    }

    #[test]
    fn tab_name_is_the_verbatim_id() {
        let item = Item::default_for_test("PWF-0001", "x");
        assert_eq!(tab_name(&item), "PWF-0001");
    }
}
