//! `LEDGER.md`: the active-handoffs summary table, reading and reconciling
//! `docs/handoffs/` entries (frontmatter + goal-checkbox counts) into rows.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::LazyLock,
};

use regex::Regex;

use super::{
    errors::{HandoffError, HandoffRead, HandoffReadStatus},
    paths::{HandoffPaths, handoff_paths},
};
use crate::{frontmatter, fs_atomic::write_text_atomic};

static CHECKBOX_ANY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^\s*-\s+\[[ xX]\]").unwrap());
static CHECKBOX_DONE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^\s*-\s+\[[xX]\]").unwrap());

pub(super) struct Row {
    pub(super) id: String,
    pub(super) title: String,
    pub(super) file_name: String,
    pub(super) goals: String,
    pub(super) created: String,
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

pub(super) struct HandoffEntry {
    pub(super) full_path: PathBuf,
    pub(super) name: String,
    pub(super) base_name: String,
    pub(super) body: String,
    pub(super) frontmatter: BTreeMap<String, String>,
}

/// *.md in dir, not LEDGER.md/README.md, with parsed frontmatter.
pub(super) fn read_handoff_entries(dir: &Path) -> Vec<HandoffEntry> {
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

pub(super) fn get_active_handoff_files(dir: &Path) -> Vec<HandoffEntry> {
    read_handoff_entries(dir)
        .into_iter()
        .filter(|e| e.frontmatter.get("status").map(String::as_str) == Some("active"))
        .collect()
}

/// Rebuild LEDGER.md from active handoffs.
pub(super) fn refresh_ledger_typed(root: &Path) -> Result<(PathBuf, usize), HandoffError> {
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
pub(super) fn archive_stranded(paths: &HandoffPaths) -> Result<(usize, Vec<String>), HandoffError> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::handoff::test_support::tempdir;

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
}
