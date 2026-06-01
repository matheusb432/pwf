//! Declarative clap command tree — the single source of truth for parsing AND
//! `--help` (PWF-0030). Doc comments on each command/arg ARE the help text; keep
//! them tight. The only argv preprocessing left is `preprocess.rs`, which injects
//! the implicit `pw` `list`/`route` subcommand defaults clap can't derive.
//!
//! The per-engine `*Common` flag groups are flattened into every action because
//! tests and scripts inject sandbox flags (`--config-path`, `--notes-dir` /
//! `--repo-root`, `--date`, …) onto individual commands — matching the old flat parser.

use crate::engines::pending_work::{Action as PendingWorkAction, PendingWorkCommand};
use clap::{Args, Parser, Subcommand};

/// pwf — pending-work / handoff / migrate engine for managed repos.
#[derive(Parser, Debug)]
#[command(name = "pwf", version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub engine: Engine,
}

/// Top-level engines.
#[derive(Subcommand, Debug)]
pub enum Engine {
    /// Track on-demand agent prompts ("pwf tasks") across managed repos.
    #[command(alias = "pending-work")]
    Pw {
        #[command(subcommand)]
        action: PwAction,
    },
    /// Per-repo handoff ledgers (resume notes between sessions).
    Handoff {
        #[command(subcommand)]
        action: HandoffAction,
    },
    /// One-shot: migrate a flat `<project>.md` note into the folder model.
    Migrate(MigrateArgs),
}

// ── pw engine ───────────────────────────────────────────────────────────────

/// Config/sandbox overrides accepted by every `pw` command (flattened).
#[derive(Args, Debug, Default)]
pub struct PwCommon {
    /// Path to the pwf config JSON (overrides $PWF_CONFIG).
    #[arg(long)]
    pub config_path: Option<String>,
    /// Override the notes directory.
    #[arg(long)]
    pub notes_dir: Option<String>,
    /// Date stamp (YYYY-MM-DD); defaults to today.
    #[arg(long)]
    pub date: Option<String>,
}

/// pending-work verbs (`pwf <verb>`).
#[derive(Subcommand, Debug)]
pub enum PwAction {
    /// Add a pwf task: `pwf add <project> "<prompt>"`.
    ///
    /// Prompt words are joined with single spaces, so quotes are optional.
    /// `--continue-handoff` / `--continue <path>` build the prompt from the
    /// repo's newest handoff or a plan path instead of positional words.
    Add {
        /// Managed project (name or unique prefix).
        #[arg(value_name = "PROJECT")]
        project: Option<String>,
        /// Task prompt words (joined with single spaces).
        #[arg(value_name = "PROMPT")]
        prompt: Vec<String>,
        /// Build the prompt from the repo's newest handoff.
        #[arg(long = "continue-handoff", conflicts_with_all = ["prompt", "continue_path"])]
        continue_handoff: bool,
        /// Build the prompt to continue the plan at PATH.
        #[arg(long = "continue", value_name = "PATH", conflicts_with_all = ["prompt", "continue_handoff"])]
        continue_path: Option<String>,
        /// File the item under a section (future|human|low-prio).
        #[arg(long, value_name = "SECTION")]
        section: Option<String>,
        /// Explicit title (else inferred from the prompt).
        #[arg(long)]
        title: Option<String>,
        /// File the item under `## Human` (shorthand for `--section human`).
        #[arg(long)]
        human: bool,
        /// Prereq item id; repeat or comma-separate for several.
        #[arg(long)]
        prereq: Vec<String>,
        #[command(flatten)]
        common: PwCommon,
    },
    /// List open normal items (scoped sections hidden unless scoped or `--all`).
    #[command(alias = "ls")]
    List {
        /// Limit to one project.
        #[arg(long)]
        project: Option<String>,
        /// Long form with per-item metadata.
        #[arg(long)]
        long: bool,
        /// Show only `## Future` items.
        #[arg(long, conflicts_with_all = ["human", "all"])]
        future: bool,
        /// Show only `## Human` items.
        #[arg(long, conflicts_with_all = ["future", "all"])]
        human: bool,
        /// Include every list section.
        #[arg(long, conflicts_with_all = ["human", "future"])]
        all: bool,
        /// Cap to N listed items (default 10; `-n 0` = all).
        #[arg(short = 'n', long, value_name = "N")]
        number: Option<usize>,
        #[command(flatten)]
        common: PwCommon,
    },
    /// Mark an item done in place, keeping a capped done-queue.
    Check {
        /// Item id (e.g. PWF-0001).
        #[arg(long)]
        id: Option<String>,
        /// Append a one-line completion report.
        #[arg(long)]
        report: Option<String>,
        /// Commit range(s) to record as provenance (repeat or comma-separate).
        #[arg(long)]
        commits: Vec<String>,
        /// Also spawn a `## Human` review task with prepped git-tools diff commands.
        #[arg(long)]
        review: bool,
        #[command(flatten)]
        common: PwCommon,
    },
    /// Mark an item cancelled in place, keeping the same capped queue as check.
    Cancel {
        /// Item id (e.g. PWF-0001).
        #[arg(long)]
        id: Option<String>,
        /// Required cancellation report: what was tried and why work stopped.
        #[arg(long)]
        report: Option<String>,
        /// Commit range(s) to record as provenance (repeat or comma-separate).
        #[arg(long)]
        commits: Vec<String>,
        /// Also spawn a `## Human` review task with prepped git-tools diff commands.
        #[arg(long)]
        review: bool,
        #[command(flatten)]
        common: PwCommon,
    },
    /// Replace an item's prompt body and/or title; append or clear its prereqs;
    /// or amend its `commits:` provenance (the only edit allowed on a closed item).
    Update {
        #[arg(long)]
        id: Option<String>,
        #[arg(long)]
        prompt: Option<String>,
        #[arg(long)]
        title: Option<String>,
        /// Prereq item id to append (repeat or comma-separate); dedups.
        #[arg(long)]
        prereq: Vec<String>,
        /// Clear all prereqs on the item.
        #[arg(long, conflicts_with = "prereq")]
        clear_prereq: bool,
        /// Overwrite the `commits:` provenance range(s) (repeat or comma-separate);
        /// works on closed done/cancelled items too.
        #[arg(long)]
        commits: Vec<String>,
        #[command(flatten)]
        common: PwCommon,
    },
    /// Print an item's note path (any status, incl. archived done/cancelled);
    /// `--show` prints the note as markdown instead.
    Resolve {
        #[arg(long)]
        id: Option<String>,
        /// Emit the task note as markdown (frontmatter minus exec-irrelevant keys + body).
        #[arg(long)]
        show: bool,
        #[command(flatten)]
        common: PwCommon,
    },
    /// Archive/clear done items.
    Clean {
        #[arg(long)]
        project: Option<String>,
        /// Bypass the confirmation guard.
        #[arg(long)]
        force: bool,
        /// Print actions without writing.
        #[arg(long)]
        dry_run: bool,
        #[command(flatten)]
        common: PwCommon,
    },
    /// Probe whether `claude` is launchable.
    Verify {
        #[arg(long)]
        id: Option<String>,
        #[command(flatten)]
        common: PwCommon,
    },
    /// Emit a launch spec for an item.
    Launch {
        #[arg(long)]
        id: Option<String>,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        thinking: Option<String>,
        #[command(flatten)]
        common: PwCommon,
    },
    /// Emit a direct `claude` launch.
    LaunchClaude {
        #[arg(long)]
        id: Option<String>,
        #[arg(long)]
        force: bool,
        #[command(flatten)]
        common: PwCommon,
    },

    // `// !` Hidden internal verbs — reachable but absent from help, matching the
    // current hand-curated help which omits route/new.
    /// Internal: word-router behind bare `pwf <words…>`.
    #[command(hide = true)]
    Route {
        /// Free-form route words (project + prompt, or a sub-verb).
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        words: Vec<String>,
        /// Long form with per-item metadata (forwarded to the list it routes to).
        #[arg(long)]
        long: bool,
        /// Show only `## Future` items (forwarded to the list it routes to).
        #[arg(long, conflicts_with_all = ["human", "all"])]
        future: bool,
        /// Show only `## Human` items (forwarded to the list it routes to).
        #[arg(long, conflicts_with_all = ["future", "all"])]
        human: bool,
        /// Include every list section (forwarded to the list it routes to).
        #[arg(long, conflicts_with_all = ["human", "future"])]
        all: bool,
        /// Cap to N listed items (forwarded to the list it routes to).
        #[arg(short = 'n', long, value_name = "N")]
        number: Option<usize>,
        #[arg(long)]
        prereq: Vec<String>,
        #[command(flatten)]
        common: PwCommon,
    },
    /// Internal: ad-hoc launch spec (no file writes).
    #[command(hide = true)]
    New {
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        prompt: Option<String>,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        thinking: Option<String>,
        #[command(flatten)]
        common: PwCommon,
    },
    /// Delete a task note and remove its index link.
    Remove {
        #[arg(long)]
        id: Option<String>,
        #[command(flatten)]
        common: PwCommon,
    },
}

// ── handoff engine ────────────────────────────────────────────────────────────

/// Config/sandbox overrides accepted by every `handoff` command (flattened).
#[derive(Args, Debug, Default)]
pub struct HandoffCommon {
    /// Path to the pwf config JSON (overrides $PWF_CONFIG).
    #[arg(long)]
    pub config_path: Option<String>,
    /// Repo root (else `git rev-parse --show-toplevel`, else cwd).
    #[arg(long)]
    pub repo_root: Option<String>,
    /// Date stamp (YYYY-MM-DD); defaults to today.
    #[arg(long)]
    pub date: Option<String>,
    /// Skip the git commit (testing/sandbox).
    #[arg(long)]
    pub no_commit: bool,
    /// Path to a pending-work allocation script (testing).
    #[arg(long)]
    pub pending_work_script: Option<String>,
}

/// handoff verbs (`pwf handoff <verb>`).
#[derive(Subcommand, Debug)]
pub enum HandoffAction {
    /// Refresh the ledger and archive stranded handoffs.
    Refresh {
        #[command(flatten)]
        common: HandoffCommon,
    },
    /// Create a handoff (allocates a pw id for managed repos).
    New {
        /// Handoff title (required).
        #[arg(long)]
        title: Option<String>,
        /// Filename slug (else derived from the title).
        #[arg(long)]
        slug: Option<String>,
        #[command(flatten)]
        common: HandoffCommon,
    },
    /// Mark a handoff done.
    Done {
        #[arg(long)]
        id: Option<String>,
        /// Completion note.
        #[arg(long)]
        reason: Option<String>,
        /// Commit range(s) to record on the linked item (repeat or comma-separate).
        #[arg(long)]
        commits: Vec<String>,
        /// Also spawn a ## Human review task for the linked item.
        #[arg(long)]
        review: bool,
        #[command(flatten)]
        common: HandoffCommon,
    },
    /// Cancel a handoff.
    Cancel {
        #[arg(long)]
        id: Option<String>,
        /// Cancellation note.
        #[arg(long)]
        reason: Option<String>,
        #[command(flatten)]
        common: HandoffCommon,
    },
    /// List handoffs.
    List {
        #[command(flatten)]
        common: HandoffCommon,
    },
}

// ── migrate engine ────────────────────────────────────────────────────────────

/// One-shot migration of a flat `<project>.md` note into the folder model.
#[derive(Args, Debug, Default)]
pub struct MigrateArgs {
    /// Path to the pwf config JSON (overrides $PWF_CONFIG).
    #[arg(long)]
    pub config_path: Option<String>,
    /// Override the notes directory.
    #[arg(long)]
    pub notes_dir: Option<String>,
    /// Date stamp (YYYY-MM-DD); defaults to today.
    #[arg(long)]
    pub date: Option<String>,
    /// Print actions without writing.
    #[arg(long)]
    pub dry_run: bool,
}

// ── bridge to the engines ─────────────────────────────────────────────────────

// `crate::cli::Args` is the engine DTO; aliased to avoid clashing with clap's
// `Args` derive imported above.
use crate::cli::Args as EngineArgs;

#[derive(Debug, Clone)]
pub enum ParsedCommand {
    PendingWork(PendingWorkCommand),
    Handoff(EngineArgs),
    Migrate(EngineArgs),
}

/// Parse a full post-binary argv into a typed engine command.
///
/// The pending-work branch preserves the clap-derived action enum all the way to
/// the engine; handoff/migrate keep using the flat DTO until their boundaries are
/// refactored.
///
/// # Errors
///
/// Returns the `clap::Error` from a parse failure, help, or version request.
pub fn parse_command_argv(argv: Vec<String>) -> Result<ParsedCommand, clap::Error> {
    let norm = crate::preprocess::normalize(argv);
    let cli = Cli::try_parse_from(std::iter::once("pwf".to_string()).chain(norm))?;
    Ok(cli.into_parsed_command())
}

/// Parse a full post-binary argv (engine + args) through subcommand-default
/// injection + clap, returning the engine name and the `Args` DTO the engines
/// consume. clap errors (incl. `--help`/`--version`) propagate as `clap::Error`.
///
/// # Errors
///
/// Returns the `clap::Error` from a parse failure, help, or version request.
pub fn parse_argv(argv: Vec<String>) -> Result<(String, EngineArgs), clap::Error> {
    let norm = crate::preprocess::normalize(argv);
    let cli = Cli::try_parse_from(std::iter::once("pwf".to_string()).chain(norm))?;
    Ok(cli.into_engine_args())
}

impl Cli {
    pub fn into_parsed_command(self) -> ParsedCommand {
        match self.engine {
            Engine::Pw { action } => ParsedCommand::PendingWork(fill_pw_command(action)),
            Engine::Handoff { action } => {
                let mut a = EngineArgs::default();
                fill_handoff(&mut a, action);
                ParsedCommand::Handoff(a)
            }
            Engine::Migrate(m) => ParsedCommand::Migrate(EngineArgs {
                config_path: m.config_path,
                notes_dir: m.notes_dir,
                date: m.date,
                dry_run: m.dry_run,
                ..Default::default()
            }),
        }
    }

    /// Flatten the parsed clap tree into the engine name + engine `Args`.
    /// `// !` Temporary bridge kept for handoff/migrate compatibility.
    pub fn into_engine_args(self) -> (String, EngineArgs) {
        let mut a = EngineArgs::default();
        let engine = match self.engine {
            Engine::Pw { action } => {
                let action = fill_pw(&mut a, action);
                a.action = Some(action.as_str().into());
                "pw"
            }
            Engine::Handoff { action } => {
                fill_handoff(&mut a, action);
                "handoff"
            }
            Engine::Migrate(m) => {
                a.config_path = m.config_path;
                a.notes_dir = m.notes_dir;
                a.date = m.date;
                a.dry_run = m.dry_run;
                "migrate"
            }
        };
        (engine.to_string(), a)
    }
}

fn fill_pw_command(action: PwAction) -> PendingWorkCommand {
    let mut a = EngineArgs::default();
    let action = fill_pw(&mut a, action);
    PendingWorkCommand::new(action, a)
}

fn apply_pw_common(a: &mut EngineArgs, c: PwCommon) {
    a.config_path = c.config_path;
    a.notes_dir = c.notes_dir;
    a.date = c.date;
}

fn normalize_pending_work_id(id: Option<String>) -> Option<String> {
    id.map(|id| id.to_ascii_uppercase())
}

fn fill_pw(a: &mut EngineArgs, action: PwAction) -> PendingWorkAction {
    match action {
        PwAction::Add {
            project,
            prompt,
            continue_handoff,
            continue_path,
            section,
            title,
            human,
            prereq,
            common,
        } => {
            a.project = project;
            a.prompt = (!prompt.is_empty()).then(|| prompt.join(" "));
            a.continue_handoff = continue_handoff;
            a.continue_path = continue_path;
            a.section = section;
            a.title = title;
            a.human = human;
            a.prereq = prereq;
            apply_pw_common(a, common);
            PendingWorkAction::Add
        }
        PwAction::List {
            project,
            long,
            future,
            human,
            all,
            number,
            common,
        } => {
            a.project = project;
            a.long = long;
            a.future = future;
            a.human = human;
            a.all = all;
            a.number = number;
            apply_pw_common(a, common);
            PendingWorkAction::List
        }
        PwAction::Check {
            id,
            report,
            commits,
            review,
            common,
        } => {
            a.id = normalize_pending_work_id(id);
            a.report = report;
            a.commits = commits;
            a.review = review;
            apply_pw_common(a, common);
            PendingWorkAction::Check
        }
        PwAction::Cancel {
            id,
            report,
            commits,
            review,
            common,
        } => {
            a.id = normalize_pending_work_id(id);
            a.report = report;
            a.commits = commits;
            a.review = review;
            apply_pw_common(a, common);
            PendingWorkAction::Cancel
        }
        PwAction::Update {
            id,
            prompt,
            title,
            prereq,
            clear_prereq,
            commits,
            common,
        } => {
            a.id = normalize_pending_work_id(id);
            a.prompt = prompt;
            a.title = title;
            a.prereq = prereq;
            a.clear_prereq = clear_prereq;
            a.commits = commits;
            apply_pw_common(a, common);
            PendingWorkAction::Update
        }
        PwAction::Resolve { id, show, common } => {
            a.id = normalize_pending_work_id(id);
            a.show = show;
            apply_pw_common(a, common);
            PendingWorkAction::Resolve
        }
        PwAction::Clean {
            project,
            force,
            dry_run,
            common,
        } => {
            a.project = project;
            a.force = force;
            a.dry_run = dry_run;
            apply_pw_common(a, common);
            PendingWorkAction::Clean
        }
        PwAction::Verify { id, common } => {
            a.id = normalize_pending_work_id(id);
            apply_pw_common(a, common);
            PendingWorkAction::Verify
        }
        PwAction::Launch {
            id,
            model,
            thinking,
            common,
        } => {
            a.id = normalize_pending_work_id(id);
            a.model = model;
            a.thinking = thinking;
            apply_pw_common(a, common);
            PendingWorkAction::Launch
        }
        PwAction::LaunchClaude { id, force, common } => {
            a.id = normalize_pending_work_id(id);
            a.force = force;
            apply_pw_common(a, common);
            PendingWorkAction::LaunchClaude
        }
        PwAction::Route {
            words,
            long,
            future,
            human,
            all,
            number,
            prereq,
            common,
        } => {
            a.words = words;
            a.long = long;
            a.future = future;
            a.human = human;
            a.all = all;
            a.number = number;
            a.prereq = prereq;
            apply_pw_common(a, common);
            PendingWorkAction::Route
        }
        PwAction::New {
            project,
            prompt,
            title,
            model,
            thinking,
            common,
        } => {
            a.project = project;
            a.prompt = prompt;
            a.title = title;
            a.model = model;
            a.thinking = thinking;
            apply_pw_common(a, common);
            PendingWorkAction::New
        }
        PwAction::Remove { id, common } => {
            a.id = normalize_pending_work_id(id);
            apply_pw_common(a, common);
            PendingWorkAction::Remove
        }
    }
}

fn apply_handoff_common(a: &mut EngineArgs, c: HandoffCommon) {
    a.config_path = c.config_path;
    a.repo_root = c.repo_root;
    a.date = c.date;
    a.no_commit = c.no_commit;
    a.pending_work_script = c.pending_work_script;
}

fn fill_handoff(a: &mut EngineArgs, action: HandoffAction) {
    match action {
        HandoffAction::Refresh { common } => {
            a.action = Some("refresh".into());
            apply_handoff_common(a, common);
        }
        HandoffAction::New {
            title,
            slug,
            common,
        } => {
            a.action = Some("new".into());
            a.title = title;
            a.slug = slug;
            apply_handoff_common(a, common);
        }
        HandoffAction::Done {
            id,
            reason,
            commits,
            review,
            common,
        } => {
            a.action = Some("done".into());
            a.id = id;
            a.reason = reason;
            a.commits = commits;
            a.review = review;
            apply_handoff_common(a, common);
        }
        HandoffAction::Cancel { id, reason, common } => {
            a.action = Some("cancel".into());
            a.id = id;
            a.reason = reason;
            apply_handoff_common(a, common);
        }
        HandoffAction::List { common } => {
            a.action = Some("list".into());
            apply_handoff_common(a, common);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    // clap's own structural validation: catches duplicate args, bad flatten,
    // invalid trailing_var_arg, etc. at test time rather than first parse.
    #[test]
    fn cli_tree_is_valid() {
        Cli::command().debug_assert();
    }

    fn pw_args(tokens: &[&str]) -> EngineArgs {
        let argv = tokens.iter().map(|s| s.to_string()).collect();
        let (engine, args) = parse_argv(argv).expect("parse");
        assert_eq!(engine, "pw");
        args
    }

    fn parse_top_level(tokens: &[&str]) -> (String, EngineArgs) {
        let argv = tokens.iter().map(|s| s.to_string()).collect();
        parse_argv(argv).expect("parse")
    }

    #[test]
    fn default_engine_when_omitted() {
        let (engine, args) = parse_top_level(&["add", "glep-shimeji", "x"]);
        assert_eq!(engine, "pw");
        assert_eq!(args.action.as_deref(), Some("add"));
        assert_eq!(args.project.as_deref(), Some("glep-shimeji"));
    }

    #[test]
    fn default_project_route_with_flags() {
        let (engine, args) = parse_top_level(&["glep-shimeji", "--long", "-n", "2"]);
        assert_eq!(engine, "pw");
        assert_eq!(args.action.as_deref(), Some("route"));
        assert_eq!(args.words, vec!["glep-shimeji"]);
        assert!(args.long);
        assert_eq!(args.number, Some(2));
    }

    // ! PWF-0041: the `pw <project>` shorthand routes through the hidden `route`
    // verb; its list flags must be forwarded, not swallowed into the route words.
    #[test]
    fn route_shorthand_forwards_long_flag() {
        let a = pw_args(&["pw", "pwf", "--long"]);
        assert_eq!(a.action.as_deref(), Some("route"));
        assert_eq!(a.words, vec!["pwf"]);
        assert!(a.long);
    }

    #[test]
    fn route_shorthand_forwards_future_flag() {
        let a = pw_args(&["pw", "pwf", "--future"]);
        assert_eq!(a.words, vec!["pwf"]);
        assert!(a.future);
    }

    #[test]
    fn route_shorthand_forwards_all_flag() {
        let a = pw_args(&["pw", "pwf", "--all"]);
        assert_eq!(a.words, vec!["pwf"]);
        assert!(a.all);
    }

    #[test]
    fn route_shorthand_forwards_combined_flags() {
        let a = pw_args(&["pw", "pwf", "--long", "--future"]);
        assert_eq!(a.words, vec!["pwf"]);
        assert!(a.long);
        assert!(a.future);
    }

    // ! PWF-0020: `-n`/`--number` must reach the list both via the `pw <project>`
    // shorthand (Route) and the explicit `pw list` verb.
    #[test]
    fn route_shorthand_forwards_number_flag() {
        let a = pw_args(&["pw", "pwf", "-n", "3"]);
        assert_eq!(a.action.as_deref(), Some("route"));
        assert_eq!(a.words, vec!["pwf"]);
        assert_eq!(a.number, Some(3));
    }

    #[test]
    fn list_parses_number_short_and_long() {
        assert_eq!(pw_args(&["pw", "list", "-n", "5"]).number, Some(5));
        assert_eq!(pw_args(&["pw", "list", "--number", "5"]).number, Some(5));
    }

    // ! PWF-0017: `pw check` accepts the new provenance flags into the DTO.
    #[test]
    fn check_parses_commits_and_review() {
        let a = pw_args(&[
            "pw",
            "check",
            "--id",
            "GLP-0001",
            "--commits",
            "a..b",
            "--commits",
            "c..d",
            "--review",
        ]);
        assert_eq!(a.action.as_deref(), Some("check"));
        assert_eq!(a.commits, vec!["a..b", "c..d"]);
        assert!(a.review);
    }

    #[test]
    fn cancel_parses_required_report_surface() {
        let a = pw_args(&[
            "pw",
            "cancel",
            "--id",
            "GLP-0001",
            "--report",
            "blocked by changed scope",
        ]);
        assert_eq!(a.action.as_deref(), Some("cancel"));
        assert_eq!(a.id.as_deref(), Some("GLP-0001"));
        assert_eq!(a.report.as_deref(), Some("blocked by changed scope"));
    }

    #[test]
    fn pending_work_id_flags_parse_to_uppercase() {
        let a = pw_args(&["pw", "check", "--id", "gLp-0001"]);
        assert_eq!(a.id.as_deref(), Some("GLP-0001"));
    }

    #[test]
    fn typed_pw_parse_keeps_action_out_of_flat_args() {
        let argv = ["pw", "check", "--id", "GLP-0001"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let ParsedCommand::PendingWork(command) = parse_command_argv(argv).expect("parse") else {
            panic!("expected pending-work command");
        };
        assert_eq!(
            command.action(),
            &crate::engines::pending_work::Action::Check
        );
        assert_eq!(command.args().id.as_deref(), Some("GLP-0001"));
        assert!(command.args().action.is_none());
    }
}
