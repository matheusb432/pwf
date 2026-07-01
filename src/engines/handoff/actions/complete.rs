//! `handoff done` / `handoff cancel` — close a handoff: flip its status,
//! archive the file, and close its linked pw item.

use std::path::{Path, PathBuf};

use regex::Regex;

use crate::{
    cli::Args,
    config,
    engines::handoff::{
        errors::HandoffError,
        ledger::{get_active_handoff_files, refresh_ledger_typed},
        paths::{get_today, handoff_paths},
        pw_bridge::{inprocess_pw_check, pw_item_is_open, spawn_pw_check},
    },
    frontmatter,
    fs_atomic::write_text_atomic,
    regexes::STATUS_LINE_RE,
};

/// Capturing `status:` variant (group `$1`); distinct from the shared,
/// non-capturing `crate::regexes::STATUS_LINE_RE`.
static STATUS_CAPTURE_RE: std::sync::LazyLock<Regex> =
    std::sync::LazyLock::new(|| Regex::new(r"(?m)^(status:.*)$").unwrap());

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

pub(in crate::engines::handoff) fn complete_handoff(
    root: &Path,
    status: &str,
    args: &Args,
) -> Result<String, HandoffError> {
    let id = args.id.as_deref().ok_or_else(|| HandoffError::MissingId {
        action: args.action.clone().unwrap_or_default(),
    })?;
    let paths = handoff_paths(root);
    let (file_path, original) = find_handoff_file(&paths.dir, id)?;
    let today = get_today(&args.date);

    // Compute the completed content — no writes until every precondition holds.
    let content = STATUS_LINE_RE
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
