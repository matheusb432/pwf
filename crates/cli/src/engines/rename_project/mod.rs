//! `pwf rename-project` — atomically relocate a managed project's pwf-db
//! identity (notes dir, `CODE-NNNN` id namespace, cross-project wikilink refs,
//! `project:` label) and its `repos.toml` `[[repo]]` entry.
//!
//! Root module is the facade: it owns the typed error, resolves the
//! [`RenameContext`] from the two manifests (pwf config → pwf-db root; repos.toml
//! → the project's path/code), enumerates items, runs the collision pre-flight,
//! and dispatches to dry-run render vs. [`apply`]. Responsibility-focused
//! children: [`plan`] (plan build + text rewriters), [`manifest`] (repos.toml
//! read + scoped edit), [`apply`] (the mutating transaction).

use std::path::{Path, PathBuf};

use crate::cli::Args;

pub mod apply;
pub mod manifest;
pub mod plan;

/// Everything that can go wrong resolving or executing a rename. Typed so the
/// tests can match exact variants; the CLI surfaces `to_string()`.
#[derive(Debug, thiserror::Error)]
pub enum RenameProjectError {
    #[error("--old <CODE> is required")]
    MissingOld,
    #[error("--new <CODE> is required")]
    MissingNew,
    #[error("nothing to do: --old equals --new and no --new-path was given")]
    NoOp,
    #[error("Missing --config-path")]
    MissingConfigPath,
    #[error("{0}")]
    Config(
        #[from]
        #[source]
        crate::config::ConfigError,
    ),
    #[error("Failed to read manifest {}: {source}", path.display())]
    ManifestRead {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("project code {0} is not registered in repos.toml")]
    UnknownCode(String),
    #[error("resolved notes dir does not exist: {}", .0.display())]
    MissingNotesDir(PathBuf),
    #[error("collision: destination already exists: {}", .0.display())]
    Collision(PathBuf),
    #[error("manifest edit failed for code {0}")]
    ManifestEdit(String),
    #[error("{op} failed for {}: {source}", path.display())]
    Io {
        op: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
}

/// The resolved inputs a rename operates on. Built by [`resolve`] from the pwf
/// config (pwf-db root) and repos.toml (the project's `path`/`code`).
pub struct RenameContext {
    pub old_code: String,
    pub new_code: String,
    pub old_path: String,
    pub new_path: String,
    pub path_changed: bool,
    /// The pwf-db vault root (`<root>/<path>` is a project's notes dir).
    pub root: PathBuf,
    pub old_folder: PathBuf,
    pub new_folder: PathBuf,
    pub old_label: String,
    pub new_label: String,
    pub manifest_path: PathBuf,
    pub manifest_text: String,
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Resolve the [`RenameContext`]: validate the code pair, read the project's
/// `path` from repos.toml, then derive the pwf-db root by reusing
/// [`crate::config::Config::notes_dir_for`] and popping the `path` components off
/// the resolved notes folder — no hardcoded vault path.
pub fn resolve(args: &Args) -> Result<RenameContext, RenameProjectError> {
    let old_code = args
        .old_code
        .clone()
        .ok_or(RenameProjectError::MissingOld)?
        .to_ascii_uppercase();
    let new_code = args
        .new_code
        .clone()
        .ok_or(RenameProjectError::MissingNew)?
        .to_ascii_uppercase();
    if new_code == old_code && args.new_path.is_none() {
        return Err(RenameProjectError::NoOp);
    }

    let manifest_path = args
        .manifest_path
        .clone()
        .map_or_else(manifest::default_manifest_path, PathBuf::from);
    let manifest_text = std::fs::read_to_string(&manifest_path).map_err(|source| {
        RenameProjectError::ManifestRead {
            path: manifest_path.clone(),
            source,
        }
    })?;
    let old_path = manifest::resolve_repo_path(&manifest_text, &old_code)
        .ok_or_else(|| RenameProjectError::UnknownCode(old_code.clone()))?;
    let new_path = args.new_path.clone().unwrap_or_else(|| old_path.clone());
    let path_changed = new_path != old_path;

    let config_path = args
        .config_path
        .clone()
        .or_else(crate::config::default_config_path)
        .ok_or(RenameProjectError::MissingConfigPath)?;
    let cfg = crate::config::load(&config_path, args.notes_dir.as_deref())?;

    let old_name = basename(&old_path);
    let notes_base = cfg.notes_dir_for(old_name);
    let old_folder = Path::new(notes_base).join(old_name);

    let mut root = old_folder.clone();
    for _ in old_path.split('/') {
        root.pop();
    }
    let new_folder = root.join(&new_path);

    if !old_folder.is_dir() {
        return Err(RenameProjectError::MissingNotesDir(old_folder));
    }

    Ok(RenameContext {
        old_label: basename(&old_path).to_string(),
        new_label: basename(&new_path).to_string(),
        old_code,
        new_code,
        old_path,
        new_path,
        path_changed,
        root,
        old_folder,
        new_folder,
        manifest_path,
        manifest_text,
    })
}

/// String-returning entrypoint for the CLI dispatch.
pub fn run(args: &Args) -> Result<String, String> {
    run_typed(args).map_err(|e| e.to_string())
}

/// Typed entrypoint: resolve → enumerate → collision pre-flight → dry-run render
/// or apply the transaction.
pub fn run_typed(args: &Args) -> Result<String, RenameProjectError> {
    let ctx = resolve(args)?;
    let items = plan::enumerate_items(&ctx.old_folder, &ctx.old_code).map_err(|source| {
        RenameProjectError::Io {
            op: "enumerate items",
            path: ctx.old_folder.clone(),
            source,
        }
    })?;
    let built = plan::build_plan(&ctx, &items);
    plan::check_collisions(&built)?;

    if args.dry_run {
        return Ok(built.render());
    }
    apply::apply_plan(&ctx, &built)?;
    Ok(built.render_summary())
}
