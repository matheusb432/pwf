//! The `Args` DTO consumed by the engines. Parsing is owned by clap (`command.rs`);
//! this is the flattened result it produces. PR2 may replace this flat struct with
//! typed per-engine inputs.

/// Color policy for `pwf session` output. Default `Auto` (TTY-gated).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorChoice {
    #[default]
    Auto,
    Always,
    Never,
}

/// Which agent `pwf session`/`pwf verify` targets (`--agent`; `-a` short only on
/// `verify` — `session`'s `-a` is `--append`). Default `Claude`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Agent {
    #[default]
    Claude,
    Codex,
}

#[allow(
    clippy::struct_excessive_bools,
    reason = "flat cross-engine CLI flag bag (see module doc: 'PR2 may replace this flat \
              struct with typed per-engine inputs'); each bool is a distinct --flag read by \
              exactly one engine, not adjacent state describing one thing multiple ways, so \
              two-variant enums would only add ceremony at every read site across the engines \
              without reducing the transposition risk the lint targets. Splitting into typed \
              per-engine structs is the real fix, tracked as the noted future PR2, not a \
              same-task rename."
)]
#[derive(Debug, Default, Clone)]
pub struct Args {
    pub action: Option<String>,
    pub config_path: Option<String>,
    pub notes_dir: Option<String>,
    pub repo_root: Option<String>,
    pub date: Option<String>,
    pub id: Option<String>,
    pub raw_id: Option<String>,
    pub project: Option<String>,
    pub prompt: Option<String>,
    pub prereq: Vec<String>,
    pub clear_prereq: bool,
    // ? add: optional effort/complexity tier (1-4); update also reads it to gate
    // ? edits_body, list reads it as an exact-match filter.
    pub effort: Option<u8>,
    // ? done: commit range(s) recorded as `commits:` provenance frontmatter (PWF-0017).
    pub commits: Vec<String>,
    // ? done: also spawn a `## Human` review task prepped with git-tools diff commands.
    pub review: bool,
    pub title: Option<String>,
    pub slug: Option<String>,
    pub reason: Option<String>,
    pub report: Option<String>,
    // ? update: free-form multi-line Markdown closeout report appended to the body
    // verbatim (closed-item safe; never reruns title/Goals regeneration).
    pub append_report: Option<String>,
    // ? update/session: rich lane-syntax bullets spliced into the body's Goals/
    // Context/Constraints/Done When sections (`-a`/`--append`); open items only.
    // `session` runs the same splice before dispatch, so the launch prompt
    // carries the extension.
    pub append: Option<String>,
    pub pending_work_script: Option<String>,
    // ? resolve: emit the note as markdown (frontmatter minus exec-irrelevant keys + body).
    pub show: bool,
    pub no_commit: bool,
    pub long: bool,
    pub force: bool,
    pub dry_run: bool,
    // ? list: cap to N items (None = default cap; Some(0) = unlimited).
    pub number: Option<usize>,
    // ? list: `-o`/`--order` raw tokens (0-2 of created|id|asc|desc), resolved to
    // an `OrderSpec` by the engine. Empty = default (created desc).
    pub order: Vec<String>,
    // ? list: include scoped sections (hidden by default).
    // ? add: `human` also routes the new item under the `## Human` section.
    pub future: bool,
    pub human: bool,
    pub all: bool,
    pub words: Vec<String>,
    // ? add: build the prompt from the repo's newest handoff / a plan path.
    pub continue_handoff: bool,
    pub continue_path: Option<String>,
    // ? add: file the item under future|human|low-prio (--human is a shorthand).
    pub section: Option<String>,
    pub color: ColorChoice,
    // ? session: skip the [Y/n] dispatch confirmation (assume yes).
    pub assume_yes: bool,
    // ? session: run the agent inline in the current terminal instead of a zellij tab.
    pub inline: bool,
    // ? session: augment the launch prompt with a git-worktree setup step.
    pub worktree: bool,
    // ? session: append an autonomy directive so the agent runs without prompting (--auto).
    pub auto: bool,
    // ? session/verify: which agent to dispatch/probe (default claude).
    pub agent: Agent,
    // ? session/verify: explicit model override (e.g. "opus", "fable"), forwarded
    // verbatim to the selected agent's `--model` flag with no validation — new
    // models ship too often to hardcode a check. Wins over effort-tier resolution.
    pub model: Option<String>,
    // ? rename-project: old/new project code, optional new repo-relative path, and
    // an override for the repos.toml manifest location.
    pub old_code: Option<String>,
    pub new_code: Option<String>,
    pub new_path: Option<String>,
    pub manifest_path: Option<String>,
}
