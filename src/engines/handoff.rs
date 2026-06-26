use std::{
    path::{Path, PathBuf},
    sync::LazyLock,
};

use regex::Regex;

use crate::{cli::Args, config, frontmatter, fs_atomic::write_text_atomic};

static SLUG_NON_ALNUM_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^a-z0-9]+").unwrap());
static CHECKBOX_ANY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^\s*-\s+\[[ xX]\]").unwrap());
static CHECKBOX_DONE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^\s*-\s+\[[xX]\]").unwrap());
static CREATED_LINE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^(created: .*)$").unwrap());
/// Capturing `status:` variant (group `$1`); distinct from the shared,
/// non-capturing `crate::regexes::STATUS_LINE_RE`.
static STATUS_CAPTURE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^(status:.*)$").unwrap());

#[derive(Debug, thiserror::Error)]
pub enum HandoffError {
    #[error("--repo-root does not exist: {root}")]
    RepoRootDoesNotExist { root: String },
    #[error("{source}")]
    CurrentDir { source: std::io::Error },
    #[error("--title is required for new.")]
    MissingTitle,
    #[error("--id is required for {action}.")]
    MissingId { action: String },
    #[error("a handoff subcommand is required.")]
    MissingSubcommand,
    #[error("unknown handoff action: {action}")]
    UnknownAction { action: String },
    #[error("Handoff already exists: {}", path.display())]
    HandoffAlreadyExists { path: PathBuf },
    #[error("No active handoff found for '{key}' in {}.", dir.display())]
    ActiveHandoffNotFound { key: String, dir: PathBuf },
    #[error("Archived handoff already exists: {}", path.display())]
    ArchiveAlreadyExists { path: PathBuf },
    #[error("Cannot remove active handoff after archiving: {source}")]
    RemoveActiveAfterArchive {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{source}")]
    CreateDir {
        action: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{source}")]
    Read {
        action: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{source}")]
    Write {
        action: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{source}")]
    Rename {
        action: &'static str,
        from: PathBuf,
        to: PathBuf,
        source: std::io::Error,
    },
    #[error("{message}")]
    PendingWork { message: String },
    #[error("pw-add output parse error (stdout: {stdout})")]
    PwAddParse { stdout: String },
    #[error("{source}")]
    SubprocessSpawn {
        operation: &'static str,
        script: String,
        source: std::io::Error,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandoffReadStatus {
    Complete,
    Degraded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandoffRead<T> {
    pub status: HandoffReadStatus,
    pub value: T,
}

impl<T> HandoffRead<T> {
    fn complete(value: T) -> Self {
        Self {
            status: HandoffReadStatus::Complete,
            value,
        }
    }

    fn degraded(value: T) -> Self {
        Self {
            status: HandoffReadStatus::Degraded,
            value,
        }
    }
}

// ── helpers ────────────────────────────────────────────────────────────────

pub fn get_today(date: &Option<String>) -> String {
    match date {
        Some(d) => d.clone(),
        None => chrono::Local::now().format("%Y-%m-%d").to_string(),
    }
}

/// Lowercase, replace non-alphanum with `-`, trim dashes.
pub fn slug(value: &str) -> String {
    let lower = value.trim().to_lowercase();
    let dashed = SLUG_NON_ALNUM_RE.replace_all(&lower, "-");
    let trimmed = dashed.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "handoff".to_string()
    } else {
        trimmed
    }
}

/// --repo-root arg, else `git rev-parse --show-toplevel`, else cwd.
pub fn repo_root(args: &Args) -> Result<PathBuf, String> {
    repo_root_typed(args).map_err(|e| e.to_string())
}

fn repo_root_typed(args: &Args) -> Result<PathBuf, HandoffError> {
    if let Some(r) = &args.repo_root {
        let p = Path::new(r);
        if !p.exists() {
            return Err(HandoffError::RepoRootDoesNotExist { root: r.clone() });
        }
        // Use the path as provided — no \\?\ prefix on Windows.
        return Ok(p.to_path_buf());
    }
    let out = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output();
    if let Ok(o) = out
        && o.status.success()
    {
        let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
        if !s.is_empty() {
            return Ok(PathBuf::from(s));
        }
    }
    std::env::current_dir().map_err(|source| HandoffError::CurrentDir { source })
}

/// Reverse-match config.projects by normalized path.
pub fn resolve_project_for_repo(root: &Path, args: &Args) -> Option<String> {
    let cfg = load_handoff_config(args);
    let cfg = cfg?;
    let norm = |p: &str| -> String { p.replace('\\', "/").trim_end_matches('/').to_lowercase() };
    let home = home();
    let root_n = norm(&root.to_string_lossy());
    for (name, path_raw) in &cfg.projects {
        let path = if path_raw == "~" {
            home.clone()
        } else if path_raw.starts_with("~/") || path_raw.starts_with("~\\") {
            Path::new(&home)
                .join(&path_raw[2..])
                .to_string_lossy()
                .into_owned()
        } else {
            path_raw.clone()
        };
        if norm(&path) == root_n {
            return Some(name.clone());
        }
    }
    None
}

fn home() -> String {
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default()
}
/// Parse the config JSON from the --config-path arg, defaulting so handoff commands
/// resolve the project without an explicit --config-path.
fn load_handoff_config(args: &Args) -> Option<config::Config> {
    load_handoff_config_typed(args).value
}

fn load_handoff_config_typed(args: &Args) -> HandoffRead<Option<config::Config>> {
    let path = args
        .config_path
        .clone()
        .or_else(config::default_config_path);
    let Some(path) = path else {
        return HandoffRead::complete(None);
    };
    match config::load(&path, None) {
        Ok(cfg) => HandoffRead::complete(Some(cfg)),
        Err(_) => HandoffRead::degraded(None),
    }
}

pub struct HandoffPaths {
    pub dir: PathBuf,
    pub archive: PathBuf,
    pub ledger: PathBuf,
}

pub fn handoff_paths(root: &Path) -> HandoffPaths {
    let dir = root.join("docs/handoffs");
    HandoffPaths {
        archive: dir.join("archived"),
        ledger: dir.join("LEDGER.md"),
        dir,
    }
}

// ── scaffold ───────────────────────────────────────────────────────────────

/// Build the markdown content for a new handoff.
pub fn scaffold(title: &str, project: &str, today: &str, pw: Option<&str>) -> String {
    let mut s = String::new();
    s.push_str("---\nstatus: active\n");
    s.push_str(&format!("project: {project}\ncreated: {today}\n"));
    if let Some(p) = pw {
        s.push_str(&format!("pw: {p}\n"));
    }
    s.push_str("---\n\n");
    s.push_str(&format!("# {title}\n\n"));
    s.push_str("## Goals\n- [ ] <task title> :: <task description>\n\n");
    s.push_str("## Context\n\n## Next steps\n-\n\n");
    s.push_str("<!-- Lifecycle: while active, this is a LIVE document \u{2014} check off Goals as you finish them.\n");
    s.push_str("     When all Goals are done run `handoff done <id>` (sets status done, checks the pw item,\n");
    s.push_str("     drops the LEDGER row, moves this file to archived/, commits). Never edit archived/. -->\n");
    s
}

// ── ledger ─────────────────────────────────────────────────────────────────

struct Row {
    id: String,
    title: String,
    file_name: String,
    goals: String,
    created: String,
}

/// Count `- [ ]` / `- [x]` checkboxes.
fn goal_count(content: &str) -> String {
    let mut total = 0usize;
    let mut done = 0usize;
    for line in content.split('\n') {
        if CHECKBOX_ANY_RE.is_match(line) {
            total += 1;
        }
        if CHECKBOX_DONE_RE.is_match(line) {
            done += 1;
        }
    }
    format!("{done}/{total}")
}

/// First `^#\s+(.+?)\s*$` line (exactly one '#' then whitespace — so `## Goals` is
/// skipped), else fallback.
fn handoff_title(content: &str, fallback: &str) -> String {
    for line in content.split('\n') {
        if let Some(rest) = line.strip_prefix('#')
            && rest.starts_with(char::is_whitespace)
        {
            let rest = rest.trim();
            if !rest.is_empty() {
                return rest.to_string();
            }
        }
    }
    fallback.to_string()
}

struct HandoffEntry {
    full_path: PathBuf,
    name: String,
    base_name: String,
    body: String,
    frontmatter: std::collections::BTreeMap<String, String>,
}

/// *.md in dir, not LEDGER.md/README.md, with parsed frontmatter.
fn read_handoff_entries(dir: &Path) -> Vec<HandoffEntry> {
    read_handoff_entries_typed(dir).value
}

fn read_handoff_entries_typed(dir: &Path) -> HandoffRead<Vec<HandoffEntry>> {
    if !dir.exists() {
        return HandoffRead::complete(Vec::new());
    }
    let mut entries = Vec::new();
    let mut status = HandoffReadStatus::Complete;
    let read = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return HandoffRead::degraded(Vec::new()),
    };
    for entry in read {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => {
                status = HandoffReadStatus::Degraded;
                continue;
            }
        };
        let p = entry.path();
        if p.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let name = p
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        let lower = name.to_lowercase();
        if lower == "ledger.md" || lower == "readme.md" {
            continue;
        }
        let content = match std::fs::read_to_string(&p) {
            Ok(c) => c,
            Err(_) => {
                status = HandoffReadStatus::Degraded;
                continue;
            }
        };
        let parsed = frontmatter::parse(&content);
        let base_name = p
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        entries.push(HandoffEntry {
            full_path: p,
            name,
            base_name,
            body: parsed.body,
            frontmatter: parsed.frontmatter,
        });
    }
    HandoffRead {
        status,
        value: entries,
    }
}

fn get_active_handoff_files(dir: &Path) -> Vec<HandoffEntry> {
    read_handoff_entries(dir)
        .into_iter()
        .filter(|e| e.frontmatter.get("status").map(String::as_str) == Some("active"))
        .collect()
}

/// Rebuild LEDGER.md from active handoffs.
pub fn refresh_ledger(root: &Path) -> Result<(PathBuf, usize), String> {
    refresh_ledger_typed(root).map_err(|e| e.to_string())
}

fn refresh_ledger_typed(root: &Path) -> Result<(PathBuf, usize), HandoffError> {
    let paths = handoff_paths(root);
    if !paths.dir.exists() {
        std::fs::create_dir_all(&paths.dir).map_err(|source| HandoffError::CreateDir {
            action: "refresh-ledger",
            path: paths.dir.clone(),
            source,
        })?;
    }
    let active = get_active_handoff_files(&paths.dir);
    let mut rows: Vec<Row> = active
        .iter()
        .map(|e| {
            let goals = goal_count(&e.body);
            let title = handoff_title(&e.body, &e.base_name);
            let id = e
                .frontmatter
                .get("pw")
                .cloned()
                .unwrap_or_else(|| e.base_name.clone());
            let created = e.frontmatter.get("created").cloned().unwrap_or_default();
            Row {
                id,
                title,
                file_name: e.name.clone(),
                goals,
                created,
            }
        })
        .collect();
    // Sort by (created, file_name) descending
    rows.sort_by(|a, b| {
        b.created
            .cmp(&a.created)
            .then_with(|| b.file_name.cmp(&a.file_name))
    });
    let count = rows.len();
    let text = ledger_text(&rows);
    write_text_atomic(&paths.ledger, &text).map_err(|source| HandoffError::Write {
        action: "refresh-ledger",
        path: paths.ledger.clone(),
        source,
    })?;
    Ok((paths.ledger, count))
}

fn ledger_text(rows: &[Row]) -> String {
    let mut l = String::new();
    l.push_str("# Handoff ledger \u{2014} active only\n\n");
    l.push_str("Only handoffs with status: active are listed.\n\n");
    l.push_str("| ID | Handoff | Goals | Created |\n| --- | --- | --- | --- |\n");
    for r in rows {
        l.push_str(&format!(
            "| {} | [{}]({}) | {} | {} |\n",
            r.id, r.title, r.file_name, r.goals, r.created
        ));
    }
    l
}

/// Sweep non-active handoffs out of the active dir (the LEDGER rebuild alone is
/// not a full reconcile — a stranded `status: done` file must also move).
/// Files without a `status:` key are left alone (could be drafts); an existing
/// archive of the same name is never overwritten — reported as a conflict.
fn archive_stranded(paths: &HandoffPaths) -> Result<(usize, Vec<String>), HandoffError> {
    let mut moved = 0usize;
    let mut conflicts = Vec::new();
    for e in read_handoff_entries(&paths.dir) {
        match e.frontmatter.get("status") {
            Some(s) if s != "active" => {}
            _ => continue,
        }
        let dest = paths.archive.join(&e.name);
        if dest.exists() {
            conflicts.push(e.name);
            continue;
        }
        if !paths.archive.exists() {
            std::fs::create_dir_all(&paths.archive).map_err(|source| HandoffError::CreateDir {
                action: "archive-stranded",
                path: paths.archive.clone(),
                source,
            })?;
        }
        std::fs::rename(&e.full_path, &dest).map_err(|source| HandoffError::Rename {
            action: "archive-stranded",
            from: e.full_path.clone(),
            to: dest,
            source,
        })?;
        moved += 1;
    }
    Ok((moved, conflicts))
}

// ── new ────────────────────────────────────────────────────────────────────

fn invoke_new(root: &Path, args: &Args) -> Result<String, HandoffError> {
    let title = args.title.as_deref().ok_or(HandoffError::MissingTitle)?;
    let today = get_today(&args.date);
    let slug_val = if let Some(s) = &args.slug {
        slug(s)
    } else {
        slug(title)
    };
    let paths = handoff_paths(root);
    if !paths.dir.exists() {
        std::fs::create_dir_all(&paths.dir).map_err(|source| HandoffError::CreateDir {
            action: "new",
            path: paths.dir.clone(),
            source,
        })?;
    }
    let file_path = paths.dir.join(format!("{today}-{slug_val}.md"));
    if file_path.exists() {
        return Err(HandoffError::HandoffAlreadyExists { path: file_path });
    }
    let project = resolve_project_for_repo(root, args);
    let project_label = project
        .clone()
        .unwrap_or_else(|| leaf_name(root).to_string());

    // Write initial scaffold without pw
    write_text_atomic(&file_path, &scaffold(title, &project_label, &today, None)).map_err(
        |source| HandoffError::Write {
            action: "new",
            path: file_path.clone(),
            source,
        },
    )?;

    // If the repo is managed, allocate a pw work-item id: spawn the injected script
    // (tests inject pw-stub.sh) or, in production, call the pending-work engine in-process.
    let pw = if let Some(proj) = project.as_deref() {
        let id = if let Some(script) = &args.pending_work_script {
            let cfg = args
                .config_path
                .clone()
                .or_else(config::default_config_path)
                .unwrap_or_default();
            spawn_pw_add(script, &cfg, &today, proj)?
        } else {
            inprocess_pw_add(args, &today, proj)?
        };
        // Insert pw: <id> after the created: line
        let content = std::fs::read_to_string(&file_path).map_err(|source| HandoffError::Read {
            action: "new",
            path: file_path.clone(),
            source,
        })?;
        let new_content = CREATED_LINE_RE
            .replace(&content, format!("$1\npw: {id}").as_str())
            .into_owned();
        write_text_atomic(&file_path, &new_content).map_err(|source| HandoffError::Write {
            action: "new",
            path: file_path.clone(),
            source,
        })?;
        Some(id)
    } else {
        None
    };

    refresh_ledger_typed(root)?;

    let mut out = format!("Created handoff {}", file_path.display());
    if let Some(p) = pw {
        out.push_str(&format!("\n  pw: {p}"));
    }
    out.push_str("\n  Now fill the Goals + Context, then run: handoff done <id> when complete.");
    Ok(out)
}

fn leaf_name(p: &Path) -> &str {
    p.file_name().and_then(|n| n.to_str()).unwrap_or("repo")
}

/// Extract the item id from `add` text output: `ADDED PWF TASK [<id>] …`.
fn parse_added_id(text: &str) -> Option<String> {
    text.lines()
        .next()
        .and_then(|l| l.split('[').nth(1))
        .and_then(|s| s.split(']').next())
        .map(|s| s.to_string())
}

/// Spawn the external `--pending-work-script` allocator with the canonical
/// `add` protocol (`<script> add --config-path <cfg> --date <today>
/// <project> --continue-handoff`) and parse the id from its stdout text.
fn spawn_pw_add(
    script: &str,
    cfg: &str,
    today: &str,
    project: &str,
) -> Result<String, HandoffError> {
    let output = std::process::Command::new(script)
        .args([
            "add",
            "--config-path",
            cfg,
            "--date",
            today,
            project,
            "--continue-handoff",
        ])
        .output()
        .map_err(|source| HandoffError::SubprocessSpawn {
            operation: "pw-add",
            script: script.to_string(),
            source,
        })?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_added_id(stdout.trim()).ok_or_else(|| HandoffError::PwAddParse {
        stdout: stdout.to_string(),
    })
}

/// Spawn the external `--pending-work-script` allocator with the canonical
/// `check` protocol (`<script> check --config-path <cfg> --id <pw> --date <today>`).
fn spawn_pw_check(
    script: &str,
    cfg: &str,
    pw: &str,
    today: &str,
    commits: &[String],
    review: bool,
) -> Result<(), HandoffError> {
    let mut argv: Vec<String> = ["check", "--config-path", cfg, "--id", pw, "--date", today]
        .iter()
        .map(|s| s.to_string())
        .collect();
    // Forward commit-range provenance + the review-task request (PWF-0017).
    for range in commits {
        argv.push("--commits".to_string());
        argv.push(range.clone());
    }
    if review {
        argv.push("--review".to_string());
    }
    std::process::Command::new(script)
        .args(&argv)
        .output()
        .map_err(|source| HandoffError::SubprocessSpawn {
            operation: "pw-check",
            script: script.to_string(),
            source,
        })?;
    Ok(())
}

/// In-process equivalent of spawn_pw_add for production (no --pending-work-script):
/// run the pending-work engine's `add <project> --continue-handoff` and read the id.
fn inprocess_pw_add(args: &Args, today: &str, project: &str) -> Result<String, HandoffError> {
    let a = crate::cli::Args {
        action: Some("add".to_string()),
        date: Some(today.to_string()),
        config_path: args
            .config_path
            .clone()
            .or_else(config::default_config_path),
        project: Some(project.to_string()),
        continue_handoff: true,
        ..Default::default()
    };
    let out = crate::engines::pending_work::run_args(&a)
        .map_err(|message| HandoffError::PendingWork { message })?;
    parse_added_id(out.trim()).ok_or_else(|| HandoffError::PwAddParse {
        stdout: out.clone(),
    })
}

/// True when the linked pw item is still open. Probe failures (missing config or
/// notes dir) fall back to "open" so the check step keeps its existing error surface.
fn pw_item_is_open(args: &Args, pw: &str) -> bool {
    let Some(path) = args
        .config_path
        .clone()
        .or_else(config::default_config_path)
    else {
        return true;
    };
    let Ok(cfg) = config::load(&path, None) else {
        return true;
    };
    crate::engines::pending_work::is_item_open(&cfg, pw).unwrap_or(true)
}

/// In-process equivalent of spawn_pw_check for production (no --pending-work-script).
fn inprocess_pw_check(args: &Args, today: &str, pw: &str) -> Result<(), HandoffError> {
    let a = crate::cli::Args {
        action: Some("check".to_string()),
        id: Some(pw.to_string()),
        date: Some(today.to_string()),
        config_path: args
            .config_path
            .clone()
            .or_else(config::default_config_path),
        // Forward commit-range provenance + the review-task request (PWF-0017).
        commits: args.commits.clone(),
        review: args.review,
        ..Default::default()
    };
    crate::engines::pending_work::run_args(&a)
        .map_err(|message| HandoffError::PendingWork { message })?;
    Ok(())
}

// ── done / cancel ──────────────────────────────────────────────────────────

/// Search active handoffs for id match (exact basename, contains, or frontmatter pw).
fn find_handoff_file(dir: &Path, key: &str) -> Result<(PathBuf, String), HandoffError> {
    let active = get_active_handoff_files(dir);
    for e in &active {
        if e.base_name == key || e.base_name.contains(key) {
            let content =
                std::fs::read_to_string(&e.full_path).map_err(|source| HandoffError::Read {
                    action: "find-handoff",
                    path: e.full_path.clone(),
                    source,
                })?;
            return Ok((e.full_path.clone(), content));
        }
        if e.frontmatter.get("pw").map(|s| s.as_str()) == Some(key) {
            let content =
                std::fs::read_to_string(&e.full_path).map_err(|source| HandoffError::Read {
                    action: "find-handoff",
                    path: e.full_path.clone(),
                    source,
                })?;
            return Ok((e.full_path.clone(), content));
        }
    }
    Err(HandoffError::ActiveHandoffNotFound {
        key: key.to_string(),
        dir: dir.to_path_buf(),
    })
}

/// Insert/replace a field after `status:`.
fn set_frontmatter_field(content: &str, field: &str, value: &str) -> String {
    let field_re = Regex::new(&format!(r"(?m)^{field}:.*$")).unwrap();
    if field_re.is_match(content) {
        field_re
            .replace(content, format!("{field}: {value}").as_str())
            .into_owned()
    } else {
        // Insert after the first status: line
        STATUS_CAPTURE_RE
            .replace(content, format!("$1\n{field}: {value}").as_str())
            .into_owned()
    }
}

fn complete_handoff(root: &Path, status: &str, args: &Args) -> Result<String, HandoffError> {
    let id = args.id.as_deref().ok_or_else(|| HandoffError::MissingId {
        action: args.action.clone().unwrap_or_default(),
    })?;
    let paths = handoff_paths(root);
    let (file_path, original) = find_handoff_file(&paths.dir, id)?;
    let today = get_today(&args.date);

    // Compute the completed content — no writes until every precondition holds.
    let content = crate::regexes::STATUS_LINE_RE
        .replace(&original, format!("status: {status}").as_str())
        .into_owned();
    // Insert/replace completed: after status:
    let content = set_frontmatter_field(&content, "completed", &today);
    // For cancel with reason: append to body
    let content = if status == "cancelled" {
        if let Some(reason) = &args.reason {
            format!("{}\n\n> Cancelled: {reason}\n", content.trim_end())
        } else {
            content
        }
    } else {
        content
    };

    // Preflight the archive destination before mutating anything.
    let file_name = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    let dest = paths.archive.join(&file_name);
    if dest.exists() {
        return Err(HandoffError::ArchiveAlreadyExists { path: dest });
    }
    if !paths.archive.exists() {
        std::fs::create_dir_all(&paths.archive).map_err(|source| HandoffError::CreateDir {
            action: "complete-handoff",
            path: paths.archive.clone(),
            source,
        })?;
    }

    let parsed = frontmatter::parse(&content);
    let pw = parsed.frontmatter.get("pw").cloned().unwrap_or_default();

    // Close the linked pw item. Already-checked (or missing) counts as success:
    // already-done is the goal state, so skip with a note instead of failing.
    let mut pw_close: Option<&str> = None;
    if !pw.is_empty() {
        if pw_item_is_open(args, &pw) {
            if let Some(script) = &args.pending_work_script {
                let cfg = args
                    .config_path
                    .clone()
                    .or_else(config::default_config_path)
                    .unwrap_or_default();
                spawn_pw_check(script, &cfg, &pw, &today, &args.commits, args.review)?;
            } else {
                inprocess_pw_check(args, &today, &pw)?;
            }
            pw_close = Some("checked");
        } else {
            pw_close = Some("skipped-already-closed");
        }
    }

    // Archive-side write: the active file is never rewritten in place, so a
    // failure can never strand a `status: done` file in the active dir.
    // The pw close above is the one non-rollbackable step; its idempotent skip
    // makes a retry safe, so rollback only needs to cover the repo mutations.
    let drop_dest = || {
        let _ = std::fs::remove_file(&dest);
    };
    write_text_atomic(&dest, &content).map_err(|source| HandoffError::Write {
        action: "complete-handoff",
        path: dest.clone(),
        source,
    })?;
    if let Err(e) = std::fs::remove_file(&file_path) {
        drop_dest();
        return Err(HandoffError::RemoveActiveAfterArchive {
            path: file_path,
            source: e,
        });
    }
    if let Err(e) = refresh_ledger_typed(root) {
        let _ = write_text_atomic(&file_path, &original);
        drop_dest();
        return Err(e);
    }

    // Commit unless --no-commit
    if !args.no_commit {
        let base_name = Path::new(&file_name)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(&file_name);
        let _ = std::process::Command::new("git")
            .args(["-C", &root.to_string_lossy(), "add", "-A", "docs/handoffs"])
            .output();
        let _ = std::process::Command::new("git")
            .args([
                "-C",
                &root.to_string_lossy(),
                "commit",
                "-m",
                &format!("docs(handoff): archive {base_name}"),
            ])
            .output();
    }

    let mut out = format!("{status} handoff {} -> {}", file_name, dest.display());
    if pw_close == Some("skipped-already-closed") {
        out.push_str(&format!("\n  note: {pw} already checked \u{2014} skipped"));
    }
    Ok(out)
}

// ── list ───────────────────────────────────────────────────────────────────

struct LedgerRead {
    exists: bool,
    content: HandoffRead<String>,
}

fn read_ledger_content(ledger: &Path) -> LedgerRead {
    let exists = ledger.exists();
    if !exists {
        return LedgerRead {
            exists,
            content: HandoffRead::complete(String::new()),
        };
    }
    match std::fs::read_to_string(ledger) {
        Ok(content) => LedgerRead {
            exists,
            content: HandoffRead::complete(content),
        },
        Err(_) => LedgerRead {
            exists,
            content: HandoffRead::degraded(String::new()),
        },
    }
}

fn invoke_list(root: &Path, _args: &Args) -> Result<String, HandoffError> {
    let paths = handoff_paths(root);
    let ledger = read_ledger_content(&paths.ledger);
    let content = ledger.content.value;
    if ledger.exists {
        Ok(content)
    } else {
        Ok("No active handoffs (LEDGER.md not found).".to_string())
    }
}

// ── dispatch ───────────────────────────────────────────────────────────────

pub fn run(args: &Args) -> Result<String, String> {
    run_typed(args).map_err(|e| e.to_string())
}

pub fn run_typed(args: &Args) -> Result<String, HandoffError> {
    let action = args
        .action
        .as_deref()
        .ok_or(HandoffError::MissingSubcommand)?;
    let root = repo_root_typed(args)?;

    match action {
        "refresh" => {
            let (archived, conflicts) = archive_stranded(&handoff_paths(&root))?;
            let (_ledger, _count) = refresh_ledger_typed(&root)?;
            let mut out = "LEDGER refreshed.".to_string();
            if archived > 0 {
                out.push_str(&format!("\n  archived {archived} stranded handoff(s)"));
            }
            for c in &conflicts {
                out.push_str(&format!(
                    "\n  conflict: {c} already exists in archived/ \u{2014} left in place"
                ));
            }
            Ok(out)
        }
        "new" => invoke_new(&root, args),
        "done" => complete_handoff(&root, "done", args),
        "cancel" => complete_handoff(&root, "cancelled", args),
        "list" => invoke_list(&root, args),
        other => Err(HandoffError::UnknownAction {
            action: other.to_string(),
        }),
    }
}

// ── tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::assert_matches;

    use super::*;

    #[test]
    fn parse_added_id_extracts_id() {
        assert_eq!(
            parse_added_id("ADDED PWF TASK [PWF-0001] pwf :: title"),
            Some("PWF-0001".to_string())
        );
        assert_eq!(parse_added_id("oops something went wrong"), None);
    }

    #[test]
    fn slug_converts_title() {
        assert_eq!(slug("Managed Flow"), "managed-flow");
        assert_eq!(slug("  Hello World!! "), "hello-world");
        assert_eq!(slug(""), "handoff");
        assert_eq!(slug("---"), "handoff");
    }

    #[test]
    fn scaffold_without_pw() {
        let s = scaffold("Managed Flow", "test-project", "2026-01-01", None);
        assert!(
            s.starts_with("---\nstatus: active\nproject: test-project\ncreated: 2026-01-01\n---\n")
        );
        assert!(!s.contains("pw:"));
        assert!(s.contains("# Managed Flow"));
        // em-dash in lifecycle comment
        assert!(s.contains('\u{2014}'));
        assert!(s.contains("- [ ] <task title> :: <task description>"));
    }

    #[test]
    fn scaffold_with_pw_inserts_after_created() {
        let s = scaffold(
            "Managed Flow",
            "test-project",
            "2026-01-01",
            Some("TST-0001"),
        );
        assert!(s.contains("created: 2026-01-01\npw: TST-0001\n---"));
    }

    #[test]
    fn goal_count_works() {
        let body = "- [x] done :: desc\n- [ ] pending :: desc\n";
        assert_eq!(goal_count(body), "1/2");
        let body2 = "- [X] also done :: desc\n";
        assert_eq!(goal_count(body2), "1/1");
    }

    #[test]
    fn handoff_title_parses_h1() {
        let body = "\n# My Title\n\nsome text\n";
        assert_eq!(handoff_title(body, "fallback"), "My Title");
        assert_eq!(handoff_title("no heading here", "fallback"), "fallback");
    }

    #[test]
    fn set_frontmatter_field_inserts_when_absent() {
        let content = "---\nstatus: active\nproject: p\ncreated: 2026-01-01\n---\n\nbody\n";
        let out = set_frontmatter_field(content, "completed", "2026-01-02");
        assert!(out.contains("status: active\ncompleted: 2026-01-02\n"));
    }

    #[test]
    fn set_frontmatter_field_replaces_when_present() {
        let content = "---\nstatus: active\ncompleted: old\nproject: p\n---\n\nbody\n";
        let out = set_frontmatter_field(content, "completed", "2026-01-02");
        assert!(out.contains("completed: 2026-01-02"));
        assert!(!out.contains("completed: old"));
    }

    #[test]
    fn ledger_text_format() {
        let rows = vec![Row {
            id: "TST-0001".to_string(),
            title: "My Title".to_string(),
            file_name: "2026-01-01-my-title.md".to_string(),
            goals: "1/2".to_string(),
            created: "2026-01-01".to_string(),
        }];
        let text = ledger_text(&rows);
        assert!(text.starts_with("# Handoff ledger \u{2014} active only\n"));
        assert!(!text.contains("handoff.ps1"));
        assert!(text.contains("Only handoffs with status: active are listed."));
        assert!(
            text.contains("| TST-0001 | [My Title](2026-01-01-my-title.md) | 1/2 | 2026-01-01 |")
        );
    }

    fn tempdir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "pwf_handoff_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn run_typed_preserves_missing_title_text_with_variant() {
        let root = tempdir();
        let args = Args {
            action: Some("new".to_string()),
            repo_root: Some(root.to_string_lossy().into_owned()),
            ..Default::default()
        };

        let err = run_typed(&args).unwrap_err();

        assert_matches!(err, HandoffError::MissingTitle);
        assert_eq!(err.to_string(), "--title is required for new.");
    }

    #[test]
    fn run_typed_preserves_missing_id_text_with_action_field() {
        let root = tempdir();
        let args = Args {
            action: Some("done".to_string()),
            repo_root: Some(root.to_string_lossy().into_owned()),
            ..Default::default()
        };

        let err = run_typed(&args).unwrap_err();

        assert_matches!(err, HandoffError::MissingId { ref action } if action == "done");
        assert_eq!(err.to_string(), "--id is required for done.");
    }

    #[test]
    fn run_typed_preserves_unknown_action_text_with_action_field() {
        let root = tempdir();
        let args = Args {
            action: Some("wat".to_string()),
            repo_root: Some(root.to_string_lossy().into_owned()),
            ..Default::default()
        };

        let err = run_typed(&args).unwrap_err();

        assert_matches!(err, HandoffError::UnknownAction { ref action } if action == "wat");
        assert_eq!(err.to_string(), "unknown handoff action: wat");
    }

    #[test]
    fn repo_root_typed_preserves_missing_root_text_with_root_field() {
        let root = tempdir().join("missing");
        let args = Args {
            repo_root: Some(root.to_string_lossy().into_owned()),
            ..Default::default()
        };

        let err = repo_root_typed(&args).unwrap_err();

        assert!(
            matches!(err, HandoffError::RepoRootDoesNotExist { ref root } if root.ends_with("missing"))
        );
        assert_eq!(
            err.to_string(),
            format!("--repo-root does not exist: {}", root.display())
        );
    }

    #[test]
    fn read_handoff_entries_reports_degraded_when_file_read_is_ignored() {
        let dir = tempdir();
        std::fs::create_dir(dir.join("2026-01-01-unreadable.md")).unwrap();

        let read = read_handoff_entries_typed(&dir);

        assert_eq!(read.status, HandoffReadStatus::Degraded);
        assert!(read.value.is_empty());
    }

    #[test]
    fn read_handoff_entries_reports_degraded_when_read_dir_is_ignored() {
        let dir = tempdir().join("not-a-directory.md");
        std::fs::write(&dir, "not a directory\n").unwrap();

        let read = read_handoff_entries_typed(&dir);

        assert_eq!(read.status, HandoffReadStatus::Degraded);
        assert!(read.value.is_empty());
    }

    #[test]
    fn read_ledger_reports_degraded_when_existing_ledger_is_unreadable() {
        let dir = tempdir();
        let ledger = dir.join("LEDGER.md");
        std::fs::create_dir(&ledger).unwrap();

        let read = read_ledger_content(&ledger);

        assert!(read.exists);
        assert_eq!(read.content.status, HandoffReadStatus::Degraded);
        assert_eq!(read.content.value, "");
    }

    #[test]
    fn load_handoff_config_reports_degraded_when_config_load_falls_back() {
        let dir = tempdir();
        let args = Args {
            config_path: Some(dir.join("missing.json").to_string_lossy().into_owned()),
            ..Default::default()
        };

        let read = load_handoff_config_typed(&args);

        assert_eq!(read.status, HandoffReadStatus::Degraded);
        assert!(read.value.is_none());
    }
}
