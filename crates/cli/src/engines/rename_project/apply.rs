//! The mutating transaction. Executed only for a real (non-dry-run) rename, in a
//! fixed order: directory move → file renames → vault-wide id-token rewrite →
//! `project:` label rewrite → scoped manifest edit. The collision pre-flight has
//! already run, so destinations are known-clear before the first write.

use std::{path::Path, process::Command};

use pwf_core::fs_atomic::write_text_atomic;
use walkdir::WalkDir;

use super::{
    RenameContext, RenameProjectError,
    plan::{
        RenamePlan, is_scannable_md, replace_id_tokens, replace_project_index_identity,
        replace_project_label,
    },
};

fn io<'a>(
    op: &'static str,
    path: &'a Path,
) -> impl FnOnce(std::io::Error) -> RenameProjectError + use<'a> {
    move |source| RenameProjectError::Io {
        op,
        path: path.to_path_buf(),
        source,
    }
}

/// Execute the whole rename transaction against the real tree.
pub fn apply_plan(ctx: &RenameContext, plan: &RenamePlan) -> Result<(), RenameProjectError> {
    if let Some((from, to)) = &plan.dir_move {
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent).map_err(io("create parent dir", parent))?;
        }
        move_dir(&ctx.root, from, to)?;
    }

    for rename in &plan.file_renames {
        std::fs::rename(&rename.from, &rename.to).map_err(io("rename note", &rename.from))?;
    }

    if let Some((from, to)) = &plan.index_rename {
        std::fs::rename(from, to).map_err(io("rename index", from))?;
    }

    rewrite_id_tokens_in_vault(ctx)?;

    let index = std::fs::read_to_string(&plan.index_identity_update)
        .map_err(io("read index", &plan.index_identity_update))?;
    let index = replace_project_index_identity(&index, &ctx.new_code, &ctx.new_label);
    write_text_atomic(&plan.index_identity_update, &index)
        .map_err(io("write index", &plan.index_identity_update))?;

    if ctx.path_changed {
        for path in &plan.label_updates {
            let text = std::fs::read_to_string(path).map_err(io("read note", path))?;
            if let Some(new_text) = replace_project_label(&text, &ctx.old_label, &ctx.new_label) {
                write_text_atomic(path, &new_text).map_err(io("write note", path))?;
            }
        }
    }

    let new_path = ctx.path_changed.then_some(ctx.new_path.as_str());
    let edited = super::manifest::edit_manifest_block(
        &ctx.manifest_text,
        &ctx.old_code,
        &ctx.new_code,
        new_path,
    )
    .ok_or_else(|| RenameProjectError::ManifestEdit(ctx.old_code.clone()))?;
    write_text_atomic(&ctx.manifest_path, &edited)
        .map_err(io("write manifest", &ctx.manifest_path))?;
    Ok(())
}

/// Whole-token `OLD-NNNN` → `NEW-NNNN` across every scannable `.md` under the
/// vault root (post-move), covering cross-project prereq wikilinks and the
/// migrated project's own internal cross-refs. `.trash/` is skipped.
fn rewrite_id_tokens_in_vault(ctx: &RenameContext) -> Result<(), RenameProjectError> {
    for entry in WalkDir::new(&ctx.root).into_iter().filter_map(Result::ok) {
        if !is_scannable_md(entry.path(), entry.file_type().is_file()) {
            continue;
        }
        let text = std::fs::read_to_string(entry.path()).map_err(io("read note", entry.path()))?;
        let (new_text, n) = replace_id_tokens(&text, &ctx.old_code, &ctx.new_code);
        if n > 0 {
            write_text_atomic(entry.path(), &new_text).map_err(io("write note", entry.path()))?;
        }
    }
    Ok(())
}

/// Move a directory: `git mv` when `root` is inside a git work tree (so git
/// records the rename), else a plain fs rename.
fn move_dir(root: &Path, from: &Path, to: &Path) -> Result<(), RenameProjectError> {
    if is_git_repo(root) {
        let status = Command::new("git")
            .arg("-C")
            .arg(root)
            .arg("mv")
            .arg(from)
            .arg(to)
            .status();
        if matches!(status, Ok(s) if s.success()) {
            return Ok(());
        }
    }
    std::fs::rename(from, to).map_err(io("move dir", from))
}

fn is_git_repo(root: &Path) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("rev-parse")
        .arg("--is-inside-work-tree")
        .output()
        .is_ok_and(|o| o.status.success() && String::from_utf8_lossy(&o.stdout).trim() == "true")
}
