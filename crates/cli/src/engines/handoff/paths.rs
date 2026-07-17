//! Resolves repository context and the `docs/handoffs` layout.

use std::{
    path::{Path, PathBuf},
    sync::LazyLock,
};

use regex::Regex;

use super::errors::{HandoffError, HandoffRead};
use crate::{cli::EngineArgs, config};

static SLUG_NON_ALNUM_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^a-z0-9]+").unwrap());

pub(super) fn get_today(date: Option<&str>) -> String {
    match date {
        Some(d) => d.to_string(),
        None => chrono::Local::now().format("%Y-%m-%d").to_string(),
    }
}

/// Converts text to a lowercase, dash-separated slug.
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

/// Resolves the repository root from `--repo-root`, Git, then the current directory.
pub(super) fn repo_root_typed(args: &EngineArgs) -> Result<PathBuf, HandoffError> {
    if let Some(r) = &args.repo_root {
        let p = Path::new(r);
        if !p.exists() {
            return Err(HandoffError::RepoRootDoesNotExist { root: r.clone() });
        }
        // Preserve the user path instead of adding a Windows extended-length prefix.
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

/// Resolves a managed project by normalized repository path.
pub(super) fn resolve_project_for_repo(root: &Path, args: &EngineArgs) -> Option<String> {
    let cfg = load_handoff_config(args);
    let cfg = cfg?;
    let norm = |p: &str| -> String { p.replace('\\', "/").trim_end_matches('/').to_lowercase() };
    let root_n = norm(&root.to_string_lossy());
    for (name, path_raw) in &cfg.projects {
        let path = expand_home(path_raw);
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

/// Expands a leading `~` path component from the current user's home directory.
pub(super) fn expand_home(raw: &str) -> String {
    let home = home();
    if raw == "~" {
        home
    } else if let Some(rest) = raw.strip_prefix("~/").or_else(|| raw.strip_prefix("~\\")) {
        Path::new(&home).join(rest).to_string_lossy().into_owned()
    } else {
        raw.to_string()
    }
}

/// Loads the configured or default manifest for handoff project resolution.
fn load_handoff_config(args: &EngineArgs) -> Option<config::Config> {
    load_handoff_config_typed(args).value
}

fn load_handoff_config_typed(args: &EngineArgs) -> HandoffRead<Option<config::Config>> {
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

pub(super) struct HandoffPaths {
    pub(super) dir: PathBuf,
    pub(super) archive: PathBuf,
    pub(super) ledger: PathBuf,
}

pub(super) fn handoff_paths(root: &Path) -> HandoffPaths {
    let dir = root.join("docs/handoffs");
    HandoffPaths {
        archive: dir.join("archived"),
        ledger: dir.join("LEDGER.md"),
        dir,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::handoff::{errors::HandoffReadStatus, test_support::tempdir};

    #[test]
    fn slug_converts_title() {
        assert_eq!(slug("Managed Flow"), "managed-flow");
        assert_eq!(slug("  Hello World!! "), "hello-world");
        assert_eq!(slug(""), "handoff");
        assert_eq!(slug("---"), "handoff");
    }

    #[test]
    fn repo_root_typed_preserves_missing_root_text_with_root_field() {
        let dir = tempdir();
        let root = dir.path().join("missing");
        let args = EngineArgs {
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
    fn load_handoff_config_reports_degraded_when_config_load_falls_back() {
        let dir = tempdir();
        let args = EngineArgs {
            config_path: Some(
                dir.path()
                    .join("missing.json")
                    .to_string_lossy()
                    .into_owned(),
            ),
            ..Default::default()
        };

        let read = load_handoff_config_typed(&args);

        assert_eq!(read.status, HandoffReadStatus::Degraded);
        assert!(read.value.is_none());
    }
}
