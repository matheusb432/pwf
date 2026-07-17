//! Defines the clap command tree used for parsing and rich help.
//! Pending-work actions are flattened into top-level commands, while `preprocess.rs`
//! supplies the implicit `route` action. Per-engine common flags are flattened into
//! each action so callers can pass sandbox overrides directly to a command.

use clap::{Args, Parser, Subcommand};
use pwf_domain::pending_work::{WorkItemStatus, WorkItemStatusFilter};
use pwf_note::{NoteCommand, NoteVerb};

use crate::engines::pending_work::{self, PendingWorkCommand};

/// `--color` choices (clap-facing; mapped to `cli::ColorChoice` in `fill_pw`).
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum ColorArg {
    Auto,
    Always,
    Never,
}

/// `--agent` choices (clap-facing; mapped to `cli::Agent` in `fill_pw`). Default claude.
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum AgentArg {
    Claude,
    Codex,
}

/// `--status` choices for pending-work lists.
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum StatusArg {
    Active,
    Done,
    Cancelled,
    All,
}

impl StatusArg {
    fn filter(self) -> WorkItemStatusFilter {
        match self {
            Self::Active => WorkItemStatusFilter::Exact(WorkItemStatus::Active),
            Self::Done => WorkItemStatusFilter::Exact(WorkItemStatus::Done),
            Self::Cancelled => WorkItemStatusFilter::Exact(WorkItemStatus::Cancelled),
            Self::All => WorkItemStatusFilter::All,
        }
    }
}

fn agent_choice(a: AgentArg) -> crate::cli::Agent {
    match a {
        AgentArg::Claude => crate::cli::Agent::Claude,
        AgentArg::Codex => crate::cli::Agent::Codex,
    }
}

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
    // Flattening keeps pending-work verbs at the top level; `pwf pw` is retired.
    #[command(flatten)]
    Pw(PwAction),
    /// Per-repo handoff ledgers (resume notes between sessions).
    Handoff {
        #[command(subcommand)]
        action: HandoffAction,
    },
    /// One-shot: migrate a flat `<project>.md` note into the folder model.
    Migrate(MigrateArgs),
    /// One-liner project notes: `pwf note <proj> [ls|add <msg>|remove <id>]`.
    Note(NoteArgs),
    /// Atomically rename a managed project's pwf-db identity + `repos.toml` entry.
    RenameProject(RenameProjectArgs),
}

// ── pw engine ───────────────────────────────────────────────────────────────

/// Config/sandbox overrides accepted by every `pw` command (flattened).
#[derive(Args, Debug, Default)]
pub struct PwCommon {
    /// Path to the pwf config JSON (overrides $`PWF_CONFIG`).
    #[arg(long)]
    pub config_path: Option<String>,
    /// Override the notes directory.
    #[arg(long)]
    pub notes_dir: Option<String>,
    /// Date stamp (YYYY-MM-DD); defaults to today.
    #[arg(long)]
    pub date: Option<String>,
}

/// The id-input surface shared by every id-facing pending-work verb: a bare
/// positional id or the `--id` flag (mutually exclusive). Flattened into each
/// verb so the positional-or-flag logic lives in exactly one place. The compact
/// split form (`cfg 57`) is collapsed to one token in `preprocess.rs` before
/// clap, and `canonical_pending_id` normalizes whatever token lands here.
#[derive(Args, Debug, Default)]
pub struct IdArg {
    /// Item id (bare positional; `--id` also accepted). E.g. `PWF-0001`, `cfg57`.
    #[arg(value_name = "ID")]
    pos: Option<String>,
    #[arg(long = "id", value_name = "ID", conflicts_with = "pos")]
    flag: Option<String>,
}

impl IdArg {
    /// The supplied id, preferring the positional; `None` if neither was given.
    fn resolve(self) -> Option<String> {
        self.pos.or(self.flag)
    }
}

/// pending-work verbs (`pwf <verb>`).
#[derive(Subcommand, Debug)]
pub enum PwAction {
    /// Add a pwf task: `pwf add <project> "<prompt>"`.
    ///
    /// Prompt words are joined with single spaces, so quotes are optional. Rich
    /// prompts use lanes: `<title> / <goal> /c <context> /n <constraint> /d <done>`.
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
        /// Discovery tag; repeat or comma-separate for several. Input accepts `snake_case` or
        /// kebab-case.
        #[arg(long, allow_hyphen_values = true)]
        tag: Vec<String>,
        /// Effort/complexity tier (1=easy .. 4=xhard); optional. Picks a Claude model
        /// via config/model-tiers.toml when the item is later dispatched with `pwf
        /// session` (codex ignores it).
        #[arg(long, value_parser = clap::value_parser!(u8).range(1..=4))]
        effort: Option<u8>,
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
        /// Show only items tagged with this exact effort/complexity tier (1-4).
        #[arg(long, value_parser = clap::value_parser!(u8).range(1..=4))]
        effort: Option<u8>,
        /// Discovery tag filter; repeat or comma-separate for several. Every requested tag must
        /// match.
        #[arg(long, allow_hyphen_values = true)]
        tag: Vec<String>,
        /// Sort key: field (created|id|project-id) and/or direction
        /// (asc|desc), each independently optional, in either order. Default:
        /// created desc, flat across every listed project. `project-id`
        /// groups by project (default asc), then newest-id-first within it —
        /// the pre-PWF-0096 default.
        #[arg(short = 'o', long, num_args = 0..=2, value_name = "ORDER", value_parser = ["created", "id", "project-id", "asc", "desc"])]
        order: Vec<String>,
        /// Filter by one lifecycle status, or include every lifecycle status.
        #[arg(long, value_enum, default_value_t = StatusArg::Active)]
        status: StatusArg,
        #[command(flatten)]
        common: PwCommon,
    },
    /// Mark an item done in place, keeping a capped done-queue.
    Done {
        #[command(flatten)]
        id: IdArg,
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
    /// Mark an item cancelled in place, keeping the same capped queue as done.
    Cancel {
        #[command(flatten)]
        id: IdArg,
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
    /// Reopen a closed item: flip done/cancelled back to active, drop its
    /// completed/commits provenance, and restore its index link.
    Reopen {
        #[command(flatten)]
        id: IdArg,
        #[command(flatten)]
        common: PwCommon,
    },
    /// Replace an item's prompt body and/or title; append or clear its prereqs;
    /// splice rich lane-syntax bullets into the body; amend its `commits:`
    /// provenance; or append a closeout report — the last two being the only edits
    /// allowed on a closed item.
    Update {
        #[command(flatten)]
        id: IdArg,
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
        /// Discovery tag; repeat or comma-separate for several. Input accepts `snake_case` or
        /// kebab-case.
        #[arg(long, allow_hyphen_values = true)]
        tag: Vec<String>,
        /// Remove all tags before applying any supplied `--tag` values.
        #[arg(long)]
        tags_clear: bool,
        /// Overwrite the `commits:` provenance range(s) (repeat or comma-separate);
        /// works on closed done/cancelled items too.
        #[arg(long)]
        commits: Vec<String>,
        /// Append a free-form, multi-line Markdown closeout report to the body
        /// verbatim, under `### Report` — never reruns title/Goals regeneration, so
        /// it is safe on closed done/cancelled items.
        #[arg(long)]
        append_report: Option<String>,
        /// Splice rich lane-syntax bullets (same syntax as `add`'s prompt) into the
        /// body's Goals/Context/Constraints/Done When sections, growing an existing
        /// section or creating a missing one; open items only.
        #[arg(short = 'a', long, conflicts_with = "prompt")]
        append: Option<String>,
        /// Set (or overwrite) the item's effort/complexity tier (1=easy .. 4=xhard).
        /// Optional; open items only, same rule as title/body/prereq edits.
        #[arg(long, value_parser = clap::value_parser!(u8).range(1..=4))]
        effort: Option<u8>,
        #[command(flatten)]
        common: PwCommon,
    },
    /// Print an item's note path (any status, incl. archived done/cancelled);
    /// `--show` prints the note as markdown instead.
    Resolve {
        #[command(flatten)]
        id: IdArg,
        /// Emit the task note as markdown (frontmatter minus exec-irrelevant keys + body).
        #[arg(long)]
        show: bool,
        #[command(flatten)]
        common: PwCommon,
    },
    /// Shorthand for `pwf resolve --show <id>`: stream a task note's markdown.
    ///
    /// The id is a bare positional — `pwf show <id>` — or `--id`.
    Show {
        #[command(flatten)]
        id: IdArg,
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
    /// Probe whether an agent is launchable.
    Verify {
        #[command(flatten)]
        id: IdArg,
        /// Which agent to probe (claude default).
        #[arg(long = "agent", short = 'a', value_enum, default_value_t = AgentArg::Claude)]
        agent: AgentArg,
        /// Explicit model override, forwarded verbatim to the agent's `--model` flag
        /// (no validation — wins over any effort-tier resolution).
        #[arg(long, short = 'm')]
        model: Option<String>,
        #[command(flatten)]
        common: PwCommon,
    },
    // The implicit router remains callable but is omitted from help.
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
        /// Filter by one lifecycle status, or include every lifecycle status.
        #[arg(long, value_enum, default_value_t = StatusArg::Active)]
        status: StatusArg,
        #[arg(long)]
        prereq: Vec<String>,
        #[command(flatten)]
        common: PwCommon,
    },
    /// Delete a task note and remove its index link.
    Remove {
        #[command(flatten)]
        id: IdArg,
        /// Skip the [Y/n] removal confirmation (assume yes).
        #[arg(long = "yes", short = 'y')]
        yes: bool,
        #[command(flatten)]
        common: PwCommon,
    },
    /// Dispatch a real agent session into the item's zellij session as a new tab.
    ///
    /// The id is a bare positional — `pwf session <id>` — or `--id`.
    Session {
        #[command(flatten)]
        id: IdArg,
        /// Color policy for the dispatch output.
        #[arg(long, value_enum, default_value_t = ColorArg::Auto)]
        color: ColorArg,
        /// Skip the [Y/n] dispatch confirmation (assume yes).
        #[arg(long = "yes", short = 'y')]
        yes: bool,
        /// Run the agent inline in the current terminal instead of a zellij tab.
        #[arg(long = "inline", short = 'i')]
        inline: bool,
        /// Tell the dispatched agent to isolate its work in a git worktree named after the item
        /// id.
        #[arg(long = "worktree", short = 'w')]
        worktree: bool,
        /// Append an autonomy directive so the agent runs without prompting the user (for
        /// unattended dispatch).
        #[arg(long = "auto")]
        auto: bool,
        /// Which agent to dispatch (claude default).
        #[arg(long = "agent", value_enum, default_value_t = AgentArg::Claude)]
        agent: AgentArg,
        /// Splice rich lane-syntax bullets (same syntax as `update -a`/`--append`) into the
        /// item's body before dispatch, growing an existing section or creating a missing one,
        /// then dispatch with the full updated prompt as usual.
        #[arg(short = 'a', long)]
        append: Option<String>,
        /// Explicit model override, forwarded verbatim to the agent's `--model` flag
        /// (no validation — wins over any effort-tier resolution).
        #[arg(long, short = 'm')]
        model: Option<String>,
        #[command(flatten)]
        common: PwCommon,
    },
}

// ── handoff engine ────────────────────────────────────────────────────────────

/// Config/sandbox overrides accepted by every `handoff` command (flattened).
#[derive(Args, Debug, Default)]
pub struct HandoffCommon {
    /// Path to the pwf config JSON (overrides $`PWF_CONFIG`).
    #[arg(long)]
    pub config_path: Option<String>,
    /// Repo root (else `git rev-parse --show-toplevel`, else cwd).
    #[arg(long)]
    pub repo_root: Option<String>,
    /// Date stamp (YYYY-MM-DD); defaults to today.
    #[arg(long)]
    pub date: Option<String>,
    /// Path to a pending-work allocation script (testing).
    #[arg(long)]
    pub pending_work_script: Option<String>,
}

/// handoff verbs (`pwf handoff <verb>`). `done`/`cancel`/`reopen`/`refresh` are
/// retired (PWF-0117, `main::retired_handoff_verb`) — a handoff-tagged pw item
/// mirrors those operations from the pw verbs instead.
#[derive(Subcommand, Debug)]
pub enum HandoffAction {
    /// Create a handoff (allocates its linked pw item).
    Add {
        /// Handoff title (required).
        #[arg(long)]
        title: Option<String>,
        /// Filename slug (else derived from the title).
        #[arg(long)]
        slug: Option<String>,
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
    /// Path to the pwf config JSON (overrides $`PWF_CONFIG`).
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

// ── rename-project engine ─────────────────────────────────────────────────────

/// Atomically relocate a managed project's pwf-db identity — notes dir, id
/// prefix, cross-project refs, `project:` label — and its `repos.toml` entry.
#[derive(Args, Debug, Default)]
pub struct RenameProjectArgs {
    /// Current project code (resolved against repos.toml). Unknown → error.
    #[arg(long, value_name = "CODE")]
    pub old: Option<String>,
    /// Target project code. Must differ from `--old` unless `--new-path` is given.
    #[arg(long, value_name = "CODE")]
    pub new: Option<String>,
    /// New repo-relative path; moves the notes dir under the pwf-db root.
    #[arg(long = "new-path", value_name = "PATH")]
    pub new_path: Option<String>,
    /// Path to repos.toml (overrides $`ARCA_ROOT` / $HOME/tools/repository).
    #[arg(long = "manifest-path", value_name = "PATH")]
    pub manifest_path: Option<String>,
    /// Path to the pwf config JSON (overrides $`PWF_CONFIG`).
    #[arg(long)]
    pub config_path: Option<String>,
    /// Override the notes directory base.
    #[arg(long)]
    pub notes_dir: Option<String>,
    /// Print the plan; mutate nothing.
    #[arg(long)]
    pub dry_run: bool,
}

// ── note engine ───────────────────────────────────────────────────────────────

/// `pwf note <proj> …` — the project is a required positional; an omitted verb
/// lists (alias for `ls`).
#[derive(Args, Debug)]
pub struct NoteArgs {
    /// Managed project (name).
    #[arg(value_name = "PROJECT")]
    pub project: String,
    #[command(subcommand)]
    pub action: Option<NoteAction>,
    #[command(flatten)]
    pub common: NoteCommon,
}

/// Config/sandbox overrides accepted by every `note` command (flattened).
#[derive(Args, Debug, Default)]
pub struct NoteCommon {
    /// Path to the pwf config JSON (overrides $`PWF_CONFIG`).
    #[arg(long)]
    pub config_path: Option<String>,
    /// Override the notes directory.
    #[arg(long)]
    pub notes_dir: Option<String>,
    /// Date stamp (YYYY-MM-DD); defaults to today.
    #[arg(long)]
    pub date: Option<String>,
}

/// note verbs (`pwf note <proj> <verb>`).
#[derive(Subcommand, Debug)]
pub enum NoteAction {
    /// List the project's notes, newest-first (default when no verb is given).
    #[command(alias = "ls")]
    List {
        /// Cap to N listed notes (default 10; `-n 0` = all).
        #[arg(short = 'n', long, value_name = "N")]
        number: Option<usize>,
    },
    /// Add a one-liner note: `pwf note <proj> add "<message>"`.
    Add {
        /// Note message words (joined with single spaces).
        #[arg(value_name = "MESSAGE", required = true)]
        message: Vec<String>,
    },
    /// Delete a note and strip its index link: `pwf note <proj> remove <id>`.
    Remove {
        /// Note id: full `PWF-NOTE-0001`, `NOTE-0001`, or a bare `1`.
        #[arg(value_name = "ID")]
        id: String,
    },
    /// Replace a note's message: `pwf note <proj> update <id> "<message>"`.
    Update {
        /// Note id: full `PWF-NOTE-0001`, `NOTE-0001`, or a bare `1`.
        #[arg(value_name = "ID")]
        id: String,
        /// Replacement note message words (joined with single spaces).
        #[arg(value_name = "MESSAGE", required = true)]
        message: Vec<String>,
    },
}

// ── bridge to the engines ─────────────────────────────────────────────────────

use crate::cli::EngineArgs;

#[derive(Debug, Clone)]
pub enum ParsedCommand {
    PendingWork(PendingWorkCommand),
    Handoff(EngineArgs),
    Migrate(EngineArgs),
    Note(NoteCommand),
    RenameProject(EngineArgs),
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
            Engine::Pw(action) => ParsedCommand::PendingWork(fill_pw_command(action)),
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
            Engine::Note(n) => ParsedCommand::Note(fill_note(n)),
            Engine::RenameProject(r) => ParsedCommand::RenameProject(EngineArgs {
                old_code: r.old,
                new_code: r.new,
                new_path: r.new_path,
                manifest_path: r.manifest_path,
                config_path: r.config_path,
                notes_dir: r.notes_dir,
                dry_run: r.dry_run,
                ..Default::default()
            }),
        }
    }

    /// Flatten the parsed clap tree into the engine name + engine `Args`.
    /// `// !` Temporary bridge kept for handoff/migrate compatibility.
    pub fn into_engine_args(self) -> (String, EngineArgs) {
        let mut a = EngineArgs::default();
        let engine = match self.engine {
            Engine::Pw(action) => {
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
            Engine::Note(_) => {
                unreachable!("note is dispatched via parse_command_argv / ParsedCommand::Note")
            }
            Engine::RenameProject(_) => {
                unreachable!("rename-project is dispatched via parse_command_argv")
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

fn fill_note(n: NoteArgs) -> NoteCommand {
    let verb = match n.action {
        None | Some(NoteAction::List { number: None }) => NoteVerb::Ls { number: None },
        Some(NoteAction::List { number }) => NoteVerb::Ls { number },
        Some(NoteAction::Add { message }) => NoteVerb::Add {
            message: message.join(" "),
        },
        Some(NoteAction::Remove { id }) => NoteVerb::Remove { id },
        Some(NoteAction::Update { id, message }) => NoteVerb::Update {
            id,
            message: message.join(" "),
        },
    };
    NoteCommand {
        project: n.project,
        verb,
        config_path: n.common.config_path,
        notes_dir: n.common.notes_dir,
        date: n.common.date,
    }
}

fn apply_pw_common(a: &mut EngineArgs, c: PwCommon) {
    a.config_path = c.config_path;
    a.notes_dir = c.notes_dir;
    a.date = c.date;
}

fn normalize_pending_work_id(id: Option<String>) -> Option<String> {
    id.map(|id| crate::engines::pending_work::canonical_pending_id(&id))
}

/// Shared body of `PwAction::Done`/`PwAction::Cancel`, which parse identically —
/// only the resulting [`pending_work::Action`] differs.
fn fill_pw_close(
    a: &mut EngineArgs,
    id: IdArg,
    report: Option<String>,
    commits: Vec<String>,
    review: bool,
    common: PwCommon,
) {
    let raw_id = id.resolve();
    a.raw_id.clone_from(&raw_id);
    a.id = normalize_pending_work_id(raw_id);
    a.report = report;
    a.commits = commits;
    a.review = review;
    apply_pw_common(a, common);
}

/// Shared body of `PwAction::Reopen`/`PwAction::Show`, which parse identically —
/// only the resulting [`pending_work::Action`] differs.
fn fill_pw_id_only(a: &mut EngineArgs, id: IdArg, common: PwCommon) {
    let raw_id = id.resolve();
    a.raw_id.clone_from(&raw_id);
    a.id = normalize_pending_work_id(raw_id);
    apply_pw_common(a, common);
}

#[allow(
    clippy::too_many_lines,
    reason = "the exhaustive match maps each clap action directly into the shared engine DTO"
)]
fn fill_pw(a: &mut EngineArgs, action: PwAction) -> pending_work::Action {
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
            tag,
            effort,
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
            a.tag = tag;
            a.effort = effort;
            apply_pw_common(a, common);
            pending_work::Action::Add
        }
        PwAction::List {
            project,
            long,
            future,
            human,
            all,
            number,
            effort,
            tag,
            order,
            status,
            common,
        } => {
            a.project = project;
            a.long = long;
            a.future = future;
            a.human = human;
            a.all = all;
            a.number = number;
            a.effort = effort;
            a.tag = tag;
            a.order = order;
            a.status_filter = status.filter();
            apply_pw_common(a, common);
            pending_work::Action::List
        }
        PwAction::Done {
            id,
            report,
            commits,
            review,
            common,
        } => {
            fill_pw_close(a, id, report, commits, review, common);
            pending_work::Action::Done
        }
        PwAction::Cancel {
            id,
            report,
            commits,
            review,
            common,
        } => {
            fill_pw_close(a, id, report, commits, review, common);
            pending_work::Action::Cancel
        }
        PwAction::Reopen { id, common } => {
            fill_pw_id_only(a, id, common);
            pending_work::Action::Reopen
        }
        PwAction::Update {
            id,
            prompt,
            title,
            prereq,
            clear_prereq,
            tag,
            tags_clear,
            commits,
            append_report,
            append,
            effort,
            common,
        } => {
            a.id = normalize_pending_work_id(id.resolve());
            a.prompt = prompt;
            a.title = title;
            a.prereq = prereq;
            a.clear_prereq = clear_prereq;
            a.tag = tag;
            a.tags_clear = tags_clear;
            a.commits = commits;
            a.append_report = append_report;
            a.append = append;
            a.effort = effort;
            apply_pw_common(a, common);
            pending_work::Action::Update
        }
        PwAction::Resolve { id, show, common } => {
            let raw_id = id.resolve();
            a.raw_id.clone_from(&raw_id);
            a.id = normalize_pending_work_id(raw_id);
            a.show = show;
            apply_pw_common(a, common);
            pending_work::Action::Resolve
        }
        PwAction::Show { id, common } => {
            fill_pw_id_only(a, id, common);
            pending_work::Action::Show
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
            pending_work::Action::Clean
        }
        PwAction::Verify {
            id,
            agent,
            model,
            common,
        } => {
            a.id = normalize_pending_work_id(id.resolve());
            a.agent = agent_choice(agent);
            a.model = model;
            apply_pw_common(a, common);
            pending_work::Action::Verify
        }
        PwAction::Route {
            words,
            long,
            future,
            human,
            all,
            number,
            status,
            prereq,
            common,
        } => {
            a.words = words;
            a.long = long;
            a.future = future;
            a.human = human;
            a.all = all;
            a.number = number;
            a.status_filter = status.filter();
            a.prereq = prereq;
            apply_pw_common(a, common);
            pending_work::Action::Route
        }
        PwAction::Remove { id, yes, common } => {
            a.id = normalize_pending_work_id(id.resolve());
            a.assume_yes = yes;
            apply_pw_common(a, common);
            pending_work::Action::Remove
        }
        PwAction::Session {
            id,
            color,
            yes,
            inline,
            worktree,
            auto,
            agent,
            append,
            model,
            common,
        } => {
            a.id = normalize_pending_work_id(id.resolve());
            a.color = match color {
                ColorArg::Auto => crate::cli::ColorChoice::Auto,
                ColorArg::Always => crate::cli::ColorChoice::Always,
                ColorArg::Never => crate::cli::ColorChoice::Never,
            };
            a.assume_yes = yes;
            a.inline = inline;
            a.worktree = worktree;
            a.auto = auto;
            a.agent = agent_choice(agent);
            a.append = append;
            a.model = model;
            apply_pw_common(a, common);
            pending_work::Action::Session
        }
    }
}

fn apply_handoff_common(a: &mut EngineArgs, c: HandoffCommon) {
    a.config_path = c.config_path;
    a.repo_root = c.repo_root;
    a.date = c.date;
    a.pending_work_script = c.pending_work_script;
}

fn fill_handoff(a: &mut EngineArgs, action: HandoffAction) {
    match action {
        HandoffAction::Add {
            title,
            slug,
            common,
        } => {
            a.action = Some("add".into());
            a.title = title;
            a.slug = slug;
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
    use clap::CommandFactory;

    use super::*;

    #[test]
    fn cli_tree_is_valid() {
        Cli::command().debug_assert();
    }

    fn pw_args(tokens: &[&str]) -> EngineArgs {
        let argv = tokens
            .iter()
            .map(std::string::ToString::to_string)
            .collect();
        let (engine, parsed_args) = parse_argv(argv).expect("parse");
        assert_eq!(engine, "pw");
        parsed_args
    }

    fn parse_top_level(tokens: &[&str]) -> (String, EngineArgs) {
        let argv = tokens
            .iter()
            .map(std::string::ToString::to_string)
            .collect();
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
    fn add_preserves_repeatable_tag_values_for_engine_normalization() {
        let add = pw_args(&[
            "add",
            "pwf",
            "x",
            "--tag",
            "SQLite,csharp-export",
            "--tag",
            "godot",
        ]);

        assert_eq!(add.tag, ["SQLite,csharp-export", "godot"]);
    }

    #[test]
    fn list_preserves_repeatable_tag_values_for_engine_normalization() {
        let list = pw_args(&["list", "--tag", "SQLite,godot", "--tag", "setup"]);

        assert_eq!(list.tag, ["SQLite,godot", "setup"]);
    }

    #[test]
    fn list_and_project_route_parse_one_status_filter() {
        use pwf_domain::pending_work::{WorkItemStatus, WorkItemStatusFilter};

        assert_eq!(
            pw_args(&["list"]).status_filter,
            WorkItemStatusFilter::Exact(WorkItemStatus::Active)
        );
        assert_eq!(
            pw_args(&["list", "--status", "done"]).status_filter,
            WorkItemStatusFilter::Exact(WorkItemStatus::Done)
        );
        assert_eq!(
            pw_args(&["pwf", "--status", "all"]).status_filter,
            WorkItemStatusFilter::All
        );
    }

    #[test]
    fn update_accepts_clear_plus_tag_as_replacement_form() {
        let update = pw_args(&["update", "PWF-0001", "--tags-clear", "--tag", "sqlite"]);

        assert!(update.tags_clear);
        assert_eq!(update.tag, ["sqlite"]);
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

    #[test]
    fn route_shorthand_forwards_long_flag() {
        let a = pw_args(&["pwf", "--long"]);
        assert_eq!(a.action.as_deref(), Some("route"));
        assert_eq!(a.words, vec!["pwf"]);
        assert!(a.long);
    }

    #[test]
    fn route_shorthand_forwards_future_flag() {
        let a = pw_args(&["pwf", "--future"]);
        assert_eq!(a.words, vec!["pwf"]);
        assert!(a.future);
    }

    #[test]
    fn route_shorthand_forwards_all_flag() {
        let a = pw_args(&["pwf", "--all"]);
        assert_eq!(a.words, vec!["pwf"]);
        assert!(a.all);
    }

    #[test]
    fn route_shorthand_forwards_combined_flags() {
        let a = pw_args(&["pwf", "--long", "--future"]);
        assert_eq!(a.words, vec!["pwf"]);
        assert!(a.long);
        assert!(a.future);
    }

    #[test]
    fn route_shorthand_forwards_number_flag() {
        let a = pw_args(&["pwf", "-n", "3"]);
        assert_eq!(a.action.as_deref(), Some("route"));
        assert_eq!(a.words, vec!["pwf"]);
        assert_eq!(a.number, Some(3));
    }

    #[test]
    fn list_parses_number_short_and_long() {
        assert_eq!(pw_args(&["list", "-n", "5"]).number, Some(5));
        assert_eq!(pw_args(&["list", "--number", "5"]).number, Some(5));
    }

    #[test]
    fn done_parses_commits_and_review() {
        let a = pw_args(&[
            "done",
            "--id",
            "GLP-0001",
            "--commits",
            "a..b",
            "--commits",
            "c..d",
            "--review",
        ]);
        assert_eq!(a.action.as_deref(), Some("done"));
        assert_eq!(a.commits, vec!["a..b", "c..d"]);
        assert!(a.review);
    }

    #[test]
    fn cancel_parses_required_report_surface() {
        let a = pw_args(&[
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
    fn session_parses_append_short_flag_into_shared_update_field() {
        let a = pw_args(&["session", "PWF-0001", "-a", "extra context"]);
        assert_eq!(a.action.as_deref(), Some("session"));
        assert_eq!(a.append.as_deref(), Some("extra context"));
    }

    #[test]
    fn session_agent_flag_is_long_only_now_that_short_is_append() {
        let a = pw_args(&["session", "PWF-0001", "--agent", "codex"]);
        assert_eq!(a.agent, crate::cli::Agent::Codex);
        assert_eq!(a.append, None);
    }

    #[test]
    fn pending_work_id_flags_parse_to_uppercase() {
        let a = pw_args(&["done", "--id", "gLp-0001"]);
        assert_eq!(a.id.as_deref(), Some("GLP-0001"));
    }

    #[test]
    fn done_accepts_bare_positional_id() {
        assert_eq!(
            pw_args(&["done", "GLP-0001"]).id.as_deref(),
            Some("GLP-0001")
        );
    }

    #[test]
    fn done_still_accepts_id_flag() {
        assert_eq!(
            pw_args(&["done", "--id", "GLP-0001"]).id.as_deref(),
            Some("GLP-0001")
        );
    }

    #[test]
    fn remove_accepts_bare_positional_id() {
        assert_eq!(
            pw_args(&["remove", "pwf-0002"]).id.as_deref(),
            Some("PWF-0002")
        );
    }

    #[test]
    fn resolve_accepts_glued_positional_id() {
        assert_eq!(
            pw_args(&["resolve", "cfg57"]).id.as_deref(),
            Some("CFG-0057")
        );
    }

    #[test]
    fn update_accepts_split_id_form() {
        assert_eq!(
            pw_args(&["update", "cfg", "57", "--title", "x"])
                .id
                .as_deref(),
            Some("CFG-0057")
        );
    }

    #[test]
    fn positional_and_id_flag_conflict() {
        let argv = ["done", "GLP-0001", "--id", "GLP-0002"]
            .iter()
            .map(std::string::ToString::to_string)
            .collect();
        assert!(parse_argv(argv).is_err());
    }

    #[test]
    fn show_parses_positional_id_to_show_action_uppercased() {
        let argv = ["show", "pwf-0001"]
            .iter()
            .map(std::string::ToString::to_string)
            .collect();
        let ParsedCommand::PendingWork(command) = parse_command_argv(argv).expect("parse") else {
            panic!("expected pending-work command");
        };
        assert_eq!(
            command.action(),
            &crate::engines::pending_work::Action::Show
        );
        assert_eq!(command.args().id.as_deref(), Some("PWF-0001"));
    }

    #[test]
    fn typed_pw_parse_keeps_action_out_of_flat_args() {
        let argv = ["done", "--id", "GLP-0001"]
            .iter()
            .map(std::string::ToString::to_string)
            .collect();
        let ParsedCommand::PendingWork(command) = parse_command_argv(argv).expect("parse") else {
            panic!("expected pending-work command");
        };
        assert_eq!(
            command.action(),
            &crate::engines::pending_work::Action::Done
        );
        assert_eq!(command.args().id.as_deref(), Some("GLP-0001"));
        assert!(command.args().action.is_none());
    }

    fn parse_note(tokens: &[&str]) -> NoteCommand {
        let argv = tokens
            .iter()
            .map(std::string::ToString::to_string)
            .collect();
        match parse_command_argv(argv).expect("parse") {
            ParsedCommand::Note(c) => c,
            other => panic!("expected note command, got {other:?}"),
        }
    }

    #[test]
    fn note_bare_project_is_ls() {
        let c = parse_note(&["note", "pwf"]);
        assert_eq!(c.project, "pwf");
        assert!(matches!(c.verb, NoteVerb::Ls { number: None }));
    }

    #[test]
    fn note_add_joins_message_words() {
        let c = parse_note(&["note", "pwf", "add", "buy", "milk"]);
        assert!(matches!(c.verb, NoteVerb::Add { ref message } if message == "buy milk"));
    }

    #[test]
    fn note_remove_takes_bare_id() {
        let c = parse_note(&["note", "pwf", "remove", "3"]);
        assert!(matches!(c.verb, NoteVerb::Remove { ref id } if id == "3"));
    }

    #[test]
    fn note_ls_number_flag() {
        let c = parse_note(&["note", "pwf", "ls", "-n", "0"]);
        assert!(matches!(c.verb, NoteVerb::Ls { number: Some(0) }));
    }
}
