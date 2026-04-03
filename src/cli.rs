//! The `Args` DTO consumed by the engines. Parsing is owned by clap (`command.rs`);
//! this is the flattened result it produces. PR2 may replace this flat struct with
//! typed per-engine inputs.

#[derive(Debug, Default, Clone)]
pub struct Args {
    pub action: Option<String>,
    pub config_path: Option<String>,
    pub notes_dir: Option<String>,
    pub repo_root: Option<String>,
    pub date: Option<String>,
    pub id: Option<String>,
    pub project: Option<String>,
    pub prompt: Option<String>,
    pub prereq: Vec<String>,
    pub clear_prereq: bool,
    // ? check: commit range(s) recorded as `commits:` provenance frontmatter (PWF-0017).
    pub commits: Vec<String>,
    // ? check: also spawn a `## Human` review task prepped with git-tools diff commands.
    pub review: bool,
    pub title: Option<String>,
    pub slug: Option<String>,
    pub reason: Option<String>,
    pub report: Option<String>,
    pub model: Option<String>,
    pub thinking: Option<String>,
    pub pending_work_script: Option<String>,
    pub json: bool,
    pub no_commit: bool,
    pub long: bool,
    pub force: bool,
    pub dry_run: bool,
    // ? list: cap to N items (None = default cap; Some(0) = unlimited).
    pub number: Option<usize>,
    // ? list: include `## Future` / `## Human` items (hidden by default).
    // ? add: `human` also routes the new item under the `## Human` section.
    pub future: bool,
    pub human: bool,
    pub words: Vec<String>,
    // ? add: build the prompt from the repo's newest handoff / a plan path.
    pub continue_handoff: bool,
    pub continue_path: Option<String>,
    // ? add: file the item under future|human|low-prio (--human is a shorthand).
    pub section: Option<String>,
}
