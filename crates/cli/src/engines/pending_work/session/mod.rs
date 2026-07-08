//! `pwf session` — spawn a real agent as a new tab in the item's per-project
//! zellij session. Dispatch logic sits behind the `MultiplexerDriver` and
//! `AgentLauncher` seams so orchestration/rendering are tested with fakes.

mod confirmation;
mod inline;
mod launcher;
mod model_tiers;
mod multiplexer;
mod render;

pub(in crate::engines::pending_work) use inline::{InlineExec, RealExec};
pub(in crate::engines::pending_work) use launcher::{AgentLauncher, launcher_for};
// The concrete launchers are referenced by name only in tests (orchestration + verify
// rendering); production code selects via `launcher_for` and holds `&dyn AgentLauncher`.
#[cfg(test)]
pub(in crate::engines::pending_work) use launcher::{ClaudeLauncher, CodexLauncher};
pub(in crate::engines::pending_work) use model_tiers::{
    ModelTiersError, resolve_model, resolve_model_for_verify,
};
pub(in crate::engines::pending_work) use multiplexer::{
    MultiplexerDriver, NewTabError, RealZellij,
};
pub(super) use render::{DispatchOutcome, render};

use super::color::use_color;
#[cfg(test)]
use crate::confirm::FakeConfirm;
use crate::{
    cli::ColorChoice,
    config::Config,
    confirm::{Confirm, DefaultAnswer, RealConfirm},
    engines::pending_work::{
        agent::probe::{AgentProbe, RealProbe},
        errors::PendingWorkError,
        launch::LaunchPolicy,
        model::Item,
        query::find_pending_item,
    },
};

/// Dispatch policy flags, bundled so `dispatch`/`run_session` take one named
/// value instead of loose scalars (no adjacent-bool transposition, and under
/// clippy's argument-count threshold). Not `Copy` — `model_override` is owned —
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

#[cfg(test)]
impl DispatchOpts {
    /// Baseline test policy: never-color, non-interactive, default launch prompt.
    /// The chainable setters flip one flag each, so a test states only what it
    /// varies and adding a field to `DispatchOpts` touches just this constructor.
    fn test() -> Self {
        DispatchOpts {
            color: ColorChoice::Never,
            assume_yes: false,
            inline: false,
            launch: LaunchPolicy::default(),
            agent: crate::cli::Agent::Claude,
            model_override: None,
        }
    }

    fn assume_yes(mut self) -> Self {
        self.assume_yes = true;
        self
    }

    fn inline(mut self) -> Self {
        self.inline = true;
        self
    }

    fn model_override(mut self, model: &str) -> Self {
        self.model_override = Some(model.to_string());
        self
    }
}

/// Real-world entrypoint for `pwf session <id>`: resolves the live claude probe
/// and zellij/agent drivers, warns once if claude is absent from this PATH, then
/// dispatches. Keeps `run.rs` a thin match arm — the wiring lives here, beside
/// the orchestration it drives.
pub(in crate::engines::pending_work) fn dispatch(
    cfg: &Config,
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
    run_session(
        cfg,
        id,
        opts,
        &RealZellij,
        launcher,
        &RealExec,
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

/// The new tab's name: the canonical item id. Agent-independent — every harness
/// runs under a tab named for the item it dispatches, so this is not part of the
/// `AgentLauncher` seam.
fn tab_name(item: &Item) -> String {
    item.id.clone()
}

/// Dispatch `id` into its project's zellij session as a new tab. Try-then-fallback:
/// attempt `new_tab`; on `SessionNotFound`, create the session and retry once.
pub(in crate::engines::pending_work) fn run_session(
    cfg: &Config,
    id: &str,
    opts: &DispatchOpts,
    driver: &dyn MultiplexerDriver,
    launcher: &dyn AgentLauncher,
    exec: &dyn InlineExec,
    confirmer: &dyn Confirm,
) -> Result<String, PendingWorkError> {
    // Validate the request BEFORE probing any external tool, so an unknown id or
    // bad repo errors deterministically even where the tool is absent (e.g. CI).
    let item = find_pending_item(cfg, id)?;
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
    // zellij liveness is only relevant to the multiplexer path.
    if !opts.inline && !driver.available() {
        return Err(PendingWorkError::ZellijNotFound);
    }
    let session = session_name_for(&item);

    // Default-yes confirmation gate: an interactive operator can abort a mistaken
    // dispatch. `--yes` skips it; a non-interactive caller proceeds without
    // prompting so the fire-and-forget path is intact.
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

#[cfg(test)]
mod tests {
    use std::{assert_matches, cell::RefCell, fs};

    use super::*;
    use crate::engines::pending_work::{
        launch::{Auto, Worktree},
        session::{inline::fake::FakeExec, multiplexer::fake::FakeMux},
    };

    /// Stage a config + one launchable item whose repo dir exists.
    fn staged() -> (tempfile::TempDir, Config) {
        let stage = tempfile::tempdir().unwrap();
        let notes = stage.path().join("notes");
        let project = notes.join("pwf");
        let repo = stage.path().join("repo");
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

    struct RecordingConfirm {
        interactive: bool,
        answer: bool,
        questions: RefCell<Vec<String>>,
    }

    impl Confirm for RecordingConfirm {
        fn interactive(&self) -> bool {
            self.interactive
        }

        fn confirm(&self, question: &str, _default: DefaultAnswer) -> bool {
            self.questions.borrow_mut().push(question.to_string());
            self.answer
        }
    }

    fn policy(worktree: bool, auto: bool) -> LaunchPolicy {
        LaunchPolicy {
            worktree: Worktree::from(worktree),
            auto: Auto::from(auto),
        }
    }

    #[test]
    fn alive_session_yields_success_one_call() {
        let (_s, cfg) = staged();
        let driver = FakeMux::new(true, vec![Ok(())]);
        let out = run_session(
            &cfg,
            "PWF-0001",
            &DispatchOpts::test().assume_yes(),
            &driver,
            &ClaudeLauncher,
            &FakeExec::ok(),
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
        let driver = FakeMux::new(true, vec![Err(NewTabError::SessionNotFound), Ok(())]);
        let out = run_session(
            &cfg,
            "PWF-0001",
            &DispatchOpts::test().assume_yes(),
            &driver,
            &ClaudeLauncher,
            &FakeExec::ok(),
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
        let driver = FakeMux::new(
            true,
            vec![
                Err(NewTabError::SessionNotFound),
                Err(NewTabError::Other("boom".into())),
            ],
        );
        let out = run_session(
            &cfg,
            "PWF-0001",
            &DispatchOpts::test().assume_yes(),
            &driver,
            &ClaudeLauncher,
            &FakeExec::ok(),
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
        let driver = FakeMux::new(false, vec![]);
        let err = run_session(
            &cfg,
            "PWF-0001",
            &DispatchOpts::test().assume_yes(),
            &driver,
            &ClaudeLauncher,
            &FakeExec::ok(),
            &FakeConfirm {
                interactive: false,
                answer: false,
            },
        )
        .unwrap_err();
        assert_matches!(err, PendingWorkError::ZellijNotFound);
    }

    #[test]
    fn interactive_decline_aborts_without_dispatch() {
        // On a TTY, answering no returns the aborted note and never touches zellij.
        let (_s, cfg) = staged();
        let driver = FakeMux::new(true, vec![]);
        let out = run_session(
            &cfg,
            "PWF-0001",
            &DispatchOpts::test(),
            &driver,
            &ClaudeLauncher,
            &FakeExec::ok(),
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
        let driver = FakeMux::new(true, vec![Ok(())]);
        let out = run_session(
            &cfg,
            "PWF-0001",
            &DispatchOpts::test(),
            &driver,
            &ClaudeLauncher,
            &FakeExec::ok(),
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
    fn interactive_confirmation_shows_dispatch_context_metadata_without_prompt_body() {
        let (_s, cfg) = staged();
        let confirm = RecordingConfirm {
            interactive: true,
            answer: true,
            questions: RefCell::new(vec![]),
        };

        let out = run_session(
            &cfg,
            "PWF-0001",
            &DispatchOpts {
                inline: true,
                launch: policy(true, true),
                agent: crate::cli::Agent::Codex,
                ..DispatchOpts::test()
            },
            &FakeMux::new(true, vec![]),
            &CodexLauncher,
            &FakeExec::ok(),
            &confirm,
        )
        .unwrap();

        assert!(out.contains("— ran inline"));
        let questions = confirm.questions.borrow();
        assert_eq!(questions.len(), 1);
        let question = &questions[0];
        assert!(question.contains("# Confirm session dispatch"));
        assert!(question.contains("task_id: PWF-0001"));
        assert!(question.contains("title: dispatch me"));
        assert!(question.contains("mode: inline"));
        assert!(question.contains("agent: codex"));
        assert!(question.contains("autonomy: yes"));
        assert!(question.contains("worktree: yes"));
        assert!(question.contains("target: current terminal"));
        assert!(
            !question.contains("do work"),
            "confirmation must not flood the TUI with the prompt body: {question}"
        );
    }

    #[test]
    fn noninteractive_auto_proceeds_without_prompting() {
        // Agentic dispatch / pipes / CI: no TTY → proceed without consulting the
        // operator, keeping the fire-and-forget path intact. A `false` answer is
        // ignored because `interactive` is false.
        let (_s, cfg) = staged();
        let driver = FakeMux::new(true, vec![Ok(())]);
        let out = run_session(
            &cfg,
            "PWF-0001",
            &DispatchOpts::test(),
            &driver,
            &ClaudeLauncher,
            &FakeExec::ok(),
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
        let driver = FakeMux::new(true, vec![Ok(())]);
        let out = run_session(
            &cfg,
            "PWF-0001",
            &DispatchOpts::test().assume_yes(),
            &driver,
            &ClaudeLauncher,
            &FakeExec::ok(),
            &FakeConfirm {
                interactive: true,
                answer: false,
            },
        )
        .unwrap();
        assert!(out.contains("— dispatched"));
        assert_eq!(driver.calls.borrow().len(), 1);
    }

    #[test]
    fn inline_runs_agent_in_current_terminal_with_guarded_argv() {
        // `-i`: the agent runs via the InlineExec seam with the launcher's argv and
        // the validated repo cwd; the multiplexer is never consulted.
        let (_s, cfg) = staged();
        let mux = FakeMux::new(true, vec![]);
        let exec = FakeExec::ok();
        let out = run_session(
            &cfg,
            "PWF-0001",
            &DispatchOpts::test().assume_yes().inline(),
            &mux,
            &ClaudeLauncher,
            &exec,
            &FakeConfirm {
                interactive: false,
                answer: false,
            },
        )
        .unwrap();
        assert!(out.contains("— ran inline"));
        assert!(
            mux.calls.borrow().is_empty(),
            "inline must not touch zellij"
        );
        let calls = exec.calls.borrow();
        assert_eq!(calls.len(), 1);
        let (argv, cwd) = &calls[0];
        assert_eq!(argv[0], "claude");
        assert!(
            argv.contains(&"--".to_string()),
            "guard present in inline argv"
        );
        assert!(
            cwd.ends_with("repo"),
            "cwd is the validated repo dir: {cwd}"
        );
    }

    #[test]
    fn explicit_model_override_reaches_launch_argv() {
        // `--model` forwards straight through to the launcher's argv, with no
        // effort tag or model-tiers.toml lookup involved at all.
        let (_s, cfg) = staged();
        let mux = FakeMux::new(true, vec![]);
        let exec = FakeExec::ok();
        run_session(
            &cfg,
            "PWF-0001",
            &DispatchOpts::test()
                .assume_yes()
                .inline()
                .model_override("fable"),
            &mux,
            &ClaudeLauncher,
            &exec,
            &FakeConfirm {
                interactive: false,
                answer: false,
            },
        )
        .unwrap();
        let calls = exec.calls.borrow();
        let (argv, _cwd) = &calls[0];
        assert!(argv.contains(&"--model".to_string()));
        assert!(argv.contains(&"fable".to_string()));
    }

    #[test]
    fn inline_decline_aborts_without_exec() {
        // On a TTY, answering no returns the aborted note and never execs.
        let (_s, cfg) = staged();
        let exec = FakeExec::ok();
        let out = run_session(
            &cfg,
            "PWF-0001",
            &DispatchOpts::test().inline(),
            &FakeMux::new(true, vec![]),
            &ClaudeLauncher,
            &exec,
            &FakeConfirm {
                interactive: true,
                answer: false,
            },
        )
        .unwrap();
        assert!(out.contains("— aborted"));
        assert!(exec.calls.borrow().is_empty());
    }

    #[test]
    fn inline_exec_failure_is_typed_error() {
        // A failed inline run (non-Unix non-zero exit, or a Unix exec error) maps
        // to InlineExecFailed (stderr, non-zero pwf exit).
        let (_s, cfg) = staged();
        let exec = FakeExec {
            result: std::cell::RefCell::new(Some(Err("boom".into()))),
            calls: std::cell::RefCell::new(vec![]),
        };
        let err = run_session(
            &cfg,
            "PWF-0001",
            &DispatchOpts::test().assume_yes().inline(),
            &FakeMux::new(true, vec![]),
            &ClaudeLauncher,
            &exec,
            &FakeConfirm {
                interactive: false,
                answer: false,
            },
        )
        .unwrap_err();
        assert_matches!(err, PendingWorkError::InlineExecFailed { .. });
    }
}
