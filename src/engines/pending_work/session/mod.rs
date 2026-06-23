//! `pwf session` — spawn a real agent as a new tab in the item's per-project
//! zellij session. Dispatch logic sits behind the `ZellijDriver` and
//! `AgentLauncher` seams so orchestration/rendering are tested with fakes.

mod confirm;
mod launcher;
mod render;
mod zellij;

#[cfg(test)]
use confirm::FakeConfirm;
pub(in crate::engines::pending_work) use confirm::{Confirm, RealConfirm};
pub(in crate::engines::pending_work) use launcher::{AgentLauncher, ClaudeLauncher};
pub(super) use render::{DispatchOutcome, render, use_color};
pub(in crate::engines::pending_work) use zellij::{NewTabError, RealZellij, ZellijDriver};

use crate::cli::ColorChoice;
use crate::config::Config;
use crate::engines::pending_work::agent::claude::{ClaudeProbe, RealProbe};
use crate::engines::pending_work::errors::PendingWorkError;
use crate::engines::pending_work::model::Item;
use crate::engines::pending_work::query::find_pending_item;

/// Real-world entrypoint for `pwf session <id>`: resolves the live claude probe
/// and zellij/agent drivers, warns once if claude is absent from this PATH, then
/// dispatches. Keeps `run.rs` a thin match arm — the wiring lives here, beside
/// the orchestration it drives.
pub(in crate::engines::pending_work) fn dispatch(
    cfg: &Config,
    id: &str,
    color: ColorChoice,
    assume_yes: bool,
) -> Result<String, PendingWorkError> {
    if !RealProbe::resolve().available() {
        eprintln!(
            "note: claude not found on PATH from here; the new tab will surface the error if it can't run."
        );
    }
    run_session(
        cfg,
        id,
        color,
        assume_yes,
        &RealZellij,
        &ClaudeLauncher,
        &RealConfirm,
    )
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

/// Dispatch `id` into its project's zellij session as a new tab. Try-then-fallback:
/// attempt `new_tab`; on `SessionNotFound`, create the session and retry once.
pub(in crate::engines::pending_work) fn run_session(
    cfg: &Config,
    id: &str,
    color: ColorChoice,
    assume_yes: bool,
    driver: &dyn ZellijDriver,
    launcher: &dyn AgentLauncher,
    confirmer: &dyn Confirm,
) -> Result<String, PendingWorkError> {
    // Validate the request BEFORE probing the external tool, so an unknown id or
    // bad repo errors deterministically even where zellij is absent (e.g. CI).
    let item = find_pending_item(cfg, id)?;
    if !item.launchable {
        return Err(PendingWorkError::NotLaunchable {
            id: item.id,
            issues: item.issues,
        });
    }
    let repo = item.repo.clone().unwrap_or_default();
    if !std::path::Path::new(&repo).is_dir() {
        return Err(PendingWorkError::RepoMissing {
            project: item.project.clone(),
            path: repo,
        });
    }
    if !driver.available() {
        return Err(PendingWorkError::ZellijNotFound);
    }
    let session = session_name_for(&item);

    // Default-yes confirmation gate: an interactive operator can abort a mistaken
    // dispatch. `--yes` skips it; a non-interactive caller (agentic dispatch,
    // pipes, CI) proceeds without prompting so the fire-and-forget path is intact.
    if !assume_yes && confirmer.interactive() {
        let question = format!(
            "Dispatch {} \"{}\" into zellij session {session}?",
            item.id, item.session
        );
        if !confirmer.confirm(&question) {
            return Ok(format!(
                "# session {} — aborted\nnothing dispatched.\n",
                item.id
            ));
        }
    }

    let tab = launcher.tab_name(&item);
    let argv = launcher.argv(&item);

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
    let agent = argv.first().cloned().unwrap_or_default();
    Ok(render(&outcome, &agent, &repo, use_color(color)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::pending_work::session::zellij::fake::FakeZellij;
    use std::fs;

    fn nanos() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    }

    /// Stage a config + one launchable item whose repo dir exists.
    fn staged() -> (std::path::PathBuf, Config) {
        let stage = std::env::temp_dir().join(format!("pwf_session_{}", nanos()));
        let notes = stage.join("notes");
        let project = notes.join("pwf");
        let repo = stage.join("repo");
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(&repo).unwrap();
        fs::write(
            project.join("PWF-0001.md"),
            "---\nstatus: active\ntitle: dispatch me\nproject: pwf\n---\n\ndo work\n",
        )
        .unwrap();
        fs::write(project.join("pwf.md"), "- [[PWF-0001|dispatch me]]\n").unwrap();
        let cfg = crate::config::from_json(
            &format!(
                r#"{{ "notesDir": "{}", "projects": {{ "pwf": "{}" }}, "prefixes": {{ "pwf": "PWF" }} }}"#,
                notes.to_string_lossy().replace('\\', "\\\\"),
                repo.to_string_lossy().replace('\\', "\\\\")
            ),
            None,
        )
        .unwrap();
        (stage, cfg)
    }

    #[test]
    fn alive_session_yields_success_one_call() {
        let (_s, cfg) = staged();
        let driver = FakeZellij::new(true, vec![Ok(())]);
        let out = run_session(
            &cfg,
            "PWF-0001",
            ColorChoice::Never,
            true,
            &driver,
            &ClaudeLauncher,
            &FakeConfirm {
                interactive: false,
                answer: false,
            },
        )
        .unwrap();
        assert!(out.contains("— dispatched"));
        assert!(out.contains("session: pwf  ·  tab: PWF-0001"));
        assert_eq!(driver.calls.borrow().len(), 1);
    }

    #[test]
    fn dead_session_creates_then_retries_yielding_fallback() {
        let (_s, cfg) = staged();
        let driver = FakeZellij::new(true, vec![Err(NewTabError::SessionNotFound), Ok(())]);
        let out = run_session(
            &cfg,
            "PWF-0001",
            ColorChoice::Never,
            true,
            &driver,
            &ClaudeLauncher,
            &FakeConfirm {
                interactive: false,
                answer: false,
            },
        )
        .unwrap();
        assert!(out.contains("— created session + dispatched"));
        assert_eq!(
            *driver.calls.borrow(),
            vec!["new_tab:pwf:PWF-0001", "ensure:pwf", "new_tab:pwf:PWF-0001"]
        );
    }

    #[test]
    fn retry_failure_yields_error_outcome() {
        let (_s, cfg) = staged();
        let driver = FakeZellij::new(
            true,
            vec![
                Err(NewTabError::SessionNotFound),
                Err(NewTabError::Other("boom".into())),
            ],
        );
        let out = run_session(
            &cfg,
            "PWF-0001",
            ColorChoice::Never,
            true,
            &driver,
            &ClaudeLauncher,
            &FakeConfirm {
                interactive: false,
                answer: false,
            },
        )
        .unwrap();
        assert!(out.contains("— failed"));
        assert!(out.contains("error: boom"));
    }

    #[test]
    fn unavailable_zellij_is_typed_error() {
        let (_s, cfg) = staged();
        let driver = FakeZellij::new(false, vec![]);
        let err = run_session(
            &cfg,
            "PWF-0001",
            ColorChoice::Never,
            true,
            &driver,
            &ClaudeLauncher,
            &FakeConfirm {
                interactive: false,
                answer: false,
            },
        )
        .unwrap_err();
        assert!(matches!(err, PendingWorkError::ZellijNotFound));
    }

    #[test]
    fn interactive_decline_aborts_without_dispatch() {
        // On a TTY, answering no returns the aborted note and never touches zellij.
        let (_s, cfg) = staged();
        let driver = FakeZellij::new(true, vec![]);
        let out = run_session(
            &cfg,
            "PWF-0001",
            ColorChoice::Never,
            false,
            &driver,
            &ClaudeLauncher,
            &FakeConfirm {
                interactive: true,
                answer: false,
            },
        )
        .unwrap();
        assert!(out.contains("— aborted"));
        assert!(out.contains("nothing dispatched"));
        assert!(driver.calls.borrow().is_empty());
    }

    #[test]
    fn interactive_accept_dispatches() {
        // On a TTY, answering yes proceeds exactly like the unconfirmed path.
        let (_s, cfg) = staged();
        let driver = FakeZellij::new(true, vec![Ok(())]);
        let out = run_session(
            &cfg,
            "PWF-0001",
            ColorChoice::Never,
            false,
            &driver,
            &ClaudeLauncher,
            &FakeConfirm {
                interactive: true,
                answer: true,
            },
        )
        .unwrap();
        assert!(out.contains("— dispatched"));
        assert_eq!(driver.calls.borrow().len(), 1);
    }

    #[test]
    fn noninteractive_auto_proceeds_without_prompting() {
        // Agentic dispatch / pipes / CI: no TTY → proceed without consulting the
        // operator, keeping the fire-and-forget path intact. A `false` answer is
        // ignored because `interactive` is false.
        let (_s, cfg) = staged();
        let driver = FakeZellij::new(true, vec![Ok(())]);
        let out = run_session(
            &cfg,
            "PWF-0001",
            ColorChoice::Never,
            false,
            &driver,
            &ClaudeLauncher,
            &FakeConfirm {
                interactive: false,
                answer: false,
            },
        )
        .unwrap();
        assert!(out.contains("— dispatched"));
        assert_eq!(driver.calls.borrow().len(), 1);
    }

    #[test]
    fn assume_yes_skips_prompt_even_when_interactive() {
        // `--yes` bypasses the gate: a declining confirmer is never consulted.
        let (_s, cfg) = staged();
        let driver = FakeZellij::new(true, vec![Ok(())]);
        let out = run_session(
            &cfg,
            "PWF-0001",
            ColorChoice::Never,
            true,
            &driver,
            &ClaudeLauncher,
            &FakeConfirm {
                interactive: true,
                answer: false,
            },
        )
        .unwrap();
        assert!(out.contains("— dispatched"));
        assert_eq!(driver.calls.borrow().len(), 1);
    }
}
