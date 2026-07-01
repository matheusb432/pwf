//! `handoff reopen` — the inverse of `done`/`cancel`: flip an archived handoff
//! back to active and reopen its linked pw item.

use std::path::{Path, PathBuf};

use regex::Regex;

use crate::{
    cli::Args,
    config,
    engines::handoff::{
        errors::HandoffError,
        ledger::{read_handoff_entries, refresh_ledger_typed},
        paths::handoff_paths,
        pw_bridge::{inprocess_pw_reopen, spawn_pw_reopen},
    },
    frontmatter,
    fs_atomic::write_text_atomic,
    regexes::STATUS_LINE_RE,
};

/// Search archived (non-active) handoffs for an id match — the inverse lookup of
/// `find_handoff_file` (in `actions::complete`), which only sees active handoffs.
fn find_archived_handoff_file(
    archive: &Path,
    key: &str,
) -> Result<(PathBuf, String), HandoffError> {
    for e in read_handoff_entries(archive) {
        // Only non-active files are reopenable; an `active` file in archived/ is
        // anomalous and left for `refresh` to reconcile.
        if e.frontmatter.get("status").map(String::as_str) == Some("active") {
            continue;
        }
        let matches = e.base_name == key
            || e.base_name.contains(key)
            || e.frontmatter.get("pw").map(String::as_str) == Some(key);
        if matches {
            let content =
                std::fs::read_to_string(&e.full_path).map_err(|source| HandoffError::Read {
                    action: "find-archived-handoff",
                    path: e.full_path.clone(),
                    source,
                })?;
            return Ok((e.full_path.clone(), content));
        }
    }
    Err(HandoffError::ArchivedHandoffNotFound {
        key: key.to_string(),
        dir: archive.to_path_buf(),
    })
}

/// Drop a frontmatter field line (and its trailing newline) entirely.
fn drop_frontmatter_field(content: &str, field: &str) -> String {
    let re = Regex::new(&format!(r"(?m)^{field}:.*\n?")).unwrap();
    re.replace(content, "").into_owned()
}

/// `handoff reopen <id>` — the inverse of `done`/`cancel`: flip the archived
/// handoff back to `active`, drop its `completed:` stamp, un-rename it out of
/// `archived/`, reopen its linked pw item (so `refresh` won't immediately
/// re-archive it), rebuild the LEDGER, and commit.
pub(in crate::engines::handoff) fn reopen_handoff(
    root: &Path,
    args: &Args,
) -> Result<String, HandoffError> {
    let id = args.id.as_deref().ok_or_else(|| HandoffError::MissingId {
        action: args.action.clone().unwrap_or_default(),
    })?;
    let paths = handoff_paths(root);
    let (archived_path, original) = find_archived_handoff_file(&paths.archive, id)?;

    // Flip status -> active and drop the completion stamp. No writes until every
    // precondition holds.
    let content = STATUS_LINE_RE
        .replace(&original, "status: active")
        .into_owned();
    let content = drop_frontmatter_field(&content, "completed");

    let file_name = archived_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    let dest = paths.dir.join(&file_name);
    if dest.exists() {
        return Err(HandoffError::HandoffAlreadyExists { path: dest });
    }
    if !paths.dir.exists() {
        std::fs::create_dir_all(&paths.dir).map_err(|source| HandoffError::CreateDir {
            action: "reopen-handoff",
            path: paths.dir.clone(),
            source,
        })?;
    }

    // Paired flip: reopen the linked pw item so a later `refresh` (which re-archives
    // handoffs whose linked item is done) keeps this handoff active. `reopen` is
    // idempotent, so an already-active linked item is a safe no-op.
    let parsed = frontmatter::parse(&content);
    let pw = parsed.frontmatter.get("pw").cloned().unwrap_or_default();
    if !pw.is_empty() {
        if let Some(script) = &args.pending_work_script {
            let cfg = args
                .config_path
                .clone()
                .or_else(config::default_config_path)
                .unwrap_or_default();
            spawn_pw_reopen(script, &cfg, &pw)?;
        } else {
            inprocess_pw_reopen(args, &pw)?;
        }
    }

    // Restore the active file, then drop the archived copy. Roll back the write if
    // the removal or ledger rebuild fails so no partial state is stranded.
    write_text_atomic(&dest, &content).map_err(|source| HandoffError::Write {
        action: "reopen-handoff",
        path: dest.clone(),
        source,
    })?;
    let drop_dest = || {
        let _ = std::fs::remove_file(&dest);
    };
    if let Err(e) = std::fs::remove_file(&archived_path) {
        drop_dest();
        return Err(HandoffError::Rename {
            action: "reopen-handoff",
            from: archived_path.clone(),
            to: dest.clone(),
            source: e,
        });
    }
    if let Err(e) = refresh_ledger_typed(root) {
        let _ = write_text_atomic(&archived_path, &original);
        drop_dest();
        return Err(e);
    }

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
                &format!("docs(handoff): reopen {base_name}"),
            ])
            .output();
    }

    let mut out = format!("reopened handoff {} -> {}", file_name, dest.display());
    if !pw.is_empty() {
        out.push_str(&format!("\n  pw: reopened {pw}"));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use std::assert_matches;

    use super::*;
    use crate::engines::handoff::test_support::tempdir;

    #[test]
    fn drop_frontmatter_field_removes_line_without_residue() {
        let content = "---\nstatus: done\ncompleted: 2026-01-02\nproject: p\n---\n\nbody\n";
        let out = drop_frontmatter_field(content, "completed");
        assert!(!out.contains("completed:"), "completed lingered: {out}");
        assert!(
            out.contains("status: done\nproject: p\n"),
            "blank residue: {out}"
        );
    }

    #[test]
    fn find_archived_handoff_matches_by_pw_and_skips_active() {
        let dir = tempdir();
        let archive = dir.join("archived");
        std::fs::create_dir_all(&archive).unwrap();
        std::fs::write(
            archive.join("2026-01-01-managed-flow.md"),
            "---\nstatus: done\ncompleted: 2026-01-02\nproject: p\ncreated: 2026-01-01\npw: TST-0001\n---\n\n# Managed Flow\n",
        )
        .unwrap();
        // An anomalous active file in archived/ must not match.
        std::fs::write(
            archive.join("2026-01-02-other.md"),
            "---\nstatus: active\nproject: p\ncreated: 2026-01-02\npw: TST-0002\n---\n\n# Other\n",
        )
        .unwrap();

        let (path, content) = find_archived_handoff_file(&archive, "TST-0001").unwrap();
        assert!(path.ends_with("2026-01-01-managed-flow.md"));
        assert!(content.contains("status: done"));

        let err = find_archived_handoff_file(&archive, "TST-0002").unwrap_err();
        assert_matches!(err, HandoffError::ArchivedHandoffNotFound { .. });
    }
}
