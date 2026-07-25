use std::path::{Path, PathBuf};

use clap::Args;

#[derive(Args, Debug, Default)]
pub struct CommonArguments {
    /// Repo root (else `git rev-parse --show-toplevel`, else cwd).
    #[arg(long)]
    pub(crate) repo_root: Option<String>,
    /// Date stamp (YYYY-MM-DD); defaults to today.
    #[arg(long)]
    pub(crate) date: Option<String>,
    /// Path to a pending-work allocation script (testing).
    #[arg(long)]
    pub(crate) pending_work_script: Option<String>,
}

pub(crate) fn date(date: Option<&str>) -> String {
    date.map_or_else(
        || chrono::Local::now().format("%Y-%m-%d").to_string(),
        str::to_string,
    )
}

pub(crate) fn repository_root(arguments: &CommonArguments) -> Result<PathBuf, HandoffError> {
    if let Some(root) = &arguments.repo_root {
        let path = Path::new(root);
        if !path.exists() {
            return Err(HandoffError::RepoRootDoesNotExist { root: root.clone() });
        }
        return Ok(path.to_path_buf());
    }
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output();
    if let Ok(output) = output
        && output.status.success()
    {
        let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !root.is_empty() {
            return Ok(PathBuf::from(root));
        }
    }
    std::env::current_dir().map_err(|source| HandoffError::CurrentDir { source })
}

#[derive(Debug, thiserror::Error)]
pub(super) enum HandoffError {
    #[error("--repo-root does not exist: {root}")]
    RepoRootDoesNotExist { root: String },
    #[error("{source}")]
    CurrentDir { source: std::io::Error },
    #[error("--title is required for add.")]
    MissingTitle,
    #[error(
        "this repo is not a managed project: {root}; handoffs require a managed project record"
    )]
    UnmanagedRepo { root: String },
    #[error("Handoff already exists: {}", path.display())]
    HandoffAlreadyExists { path: PathBuf },
    #[error("{source}")]
    Add {
        #[source]
        source: pwf_application::handoff::add::AddHandoffError,
    },
    #[error("{source}")]
    List {
        #[source]
        source: pwf_application::handoff::list::ListHandoffsError,
    },
}
