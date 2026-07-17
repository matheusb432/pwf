//! Removes checked work-item links after stamping their backing notes as completed.

use std::{
    fmt::Write,
    path::{Path, PathBuf},
    sync::LazyLock,
};

use regex::Regex;

use super::pending_work::{project_index_path, set_status_text};
use crate::{
    config::Config,
    confirm::{Confirmation, DefaultAnswer},
    frontmatter, fs_atomic,
};

static DONE_INDEX_LINK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)^[ \t]*-[ \t]*\[[xX]\][ \t]*\[\[(?P<id>[A-Z]{2,4}-\d{4})(?:\|[^\]]*)?\]\][^\r\n]*\r?\n?",
    )
    .unwrap()
});

#[derive(Debug, thiserror::Error)]
pub enum CleanError {
    #[error("Notes directory not found: {path}")]
    NotesDirectoryNotFound { path: String },
    #[error("Cannot read index: {source}")]
    ReadIndex {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("Cannot read item file: {source}")]
    ReadItemFile {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("No TTY to confirm a clean. Re-run with --dry-run to preview or --force to apply.")]
    NoTtyToConfirm,
    #[error("Cannot write {}: {source}", path.display())]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
}

/// Identifies a checked work-item link and its complete line span in an index note.
#[derive(Debug, Clone, PartialEq)]
pub struct DoneLink {
    pub id: String,
    pub completed: Option<String>,
    pub start: usize,
    pub end: usize,
}

/// Finds checked wikilinks while ignoring open, bare, and non-wikilink entries.
///
/// # Panics
/// Panics if a regex match lacks its required whole-match capture.
pub fn find_done_index_links(content: &str) -> Vec<DoneLink> {
    DONE_INDEX_LINK_RE
        .captures_iter(content)
        .map(|m| {
            let whole = m.get(0).unwrap();
            let completed = crate::regexes::DATE_STAMP_RE
                .captures(whole.as_str())
                .map(|c| c[1].to_string());
            DoneLink {
                id: m["id"].to_string(),
                completed,
                start: whole.start(),
                end: whole.end(),
            }
        })
        .collect()
}

/// Resolves the completion date from the index stamp, frontmatter, then caller fallback.
/// Empty values are ignored.
pub fn resolve_completed(
    link_completed: Option<&str>,
    fm_completed: Option<&str>,
    fallback: &str,
) -> String {
    link_completed
        .filter(|s| !s.trim().is_empty())
        .or(fm_completed.filter(|s| !s.trim().is_empty()))
        .unwrap_or(fallback)
        .to_string()
}

/// Reports the outcome for one checked item.
#[derive(Debug, Clone)]
pub struct CleanResult {
    pub id: String,
    pub project: String,
    pub completed: Option<String>,
    pub note: String,
    pub item_file: String,
    pub status: String,
    pub issue: Option<String>,
}

fn remove_spans(content: &str, spans: &[(usize, usize)]) -> String {
    let mut out = String::with_capacity(content.len());
    let mut cursor = 0;
    for &(s, e) in spans {
        if s >= cursor {
            out.push_str(&content[cursor..s]);
            cursor = e;
        }
    }
    out.push_str(&content[cursor..]);
    out
}

struct PendingWrite {
    path: PathBuf,
    content: String,
}

struct ProjectPlan {
    project: String,
    results: Vec<CleanResult>,
    writes: Vec<PendingWrite>,
}

fn plan_project(project: &str, index_path: &Path, date: &str) -> Result<ProjectPlan, CleanError> {
    let content = std::fs::read_to_string(index_path).map_err(|source| CleanError::ReadIndex {
        path: index_path.to_path_buf(),
        source,
    })?;
    let dir = index_path.parent().unwrap_or(Path::new("."));
    let note = index_path.to_string_lossy().into_owned();

    let mut results = Vec::new();
    let mut writes = Vec::new();
    let mut remove: Vec<(usize, usize)> = Vec::new();

    for link in find_done_index_links(&content) {
        let item_path = dir.join(format!("{}.md", link.id));
        let item_file = item_path.to_string_lossy().into_owned();
        if !item_path.exists() {
            results.push(CleanResult {
                id: link.id.clone(),
                project: project.to_string(),
                completed: None,
                note: note.clone(),
                item_file,
                status: "skipped".to_string(),
                issue: Some(format!("Work-item note missing: {}", item_path.display())),
            });
            continue;
        }
        let raw =
            std::fs::read_to_string(&item_path).map_err(|source| CleanError::ReadItemFile {
                path: item_path.clone(),
                source,
            })?;
        let fm_completed = frontmatter::parse(&raw)
            .frontmatter
            .get("completed")
            .cloned();
        let completed = resolve_completed(link.completed.as_deref(), fm_completed.as_deref(), date);
        // Stamp the note before unlinking it so failures cannot orphan an unstamped item.
        writes.push(PendingWrite {
            path: item_path,
            content: set_status_text(&raw, "done", &completed),
        });
        remove.push((link.start, link.end));
        results.push(CleanResult {
            id: link.id,
            project: project.to_string(),
            completed: Some(completed),
            note: note.clone(),
            item_file,
            status: "wouldClean".to_string(),
            issue: None,
        });
    }

    if !remove.is_empty() {
        writes.push(PendingWrite {
            path: index_path.to_path_buf(),
            content: remove_spans(&content, &remove),
        });
    }

    Ok(ProjectPlan {
        project: project.to_string(),
        results,
        writes,
    })
}

fn render_text(plans: &[ProjectPlan], dry_run: bool) -> String {
    let mut out = String::new();
    for p in plans {
        if p.results.is_empty() {
            continue;
        }
        let cleaned = p.results.iter().filter(|r| r.status != "skipped").count();
        let skipped = p.results.iter().filter(|r| r.status == "skipped").count();
        let verb = if dry_run { "WOULD CLEAN" } else { "CLEANED" };
        let label = if dry_run { "would clean" } else { "cleaned" };
        let _ = writeln!(
            out,
            "{verb} {}: {cleaned} {label}, {skipped} skipped",
            p.project
        );
        for r in &p.results {
            if r.status == "skipped" {
                let _ = writeln!(
                    out,
                    "  {} skipped ({})",
                    r.id,
                    r.issue.as_deref().unwrap_or("skipped")
                );
            } else {
                let _ = writeln!(
                    out,
                    "  {} done (completed {})",
                    r.id,
                    r.completed.as_deref().unwrap_or("")
                );
            }
        }
    }
    out
}

/// Cleans one resolved project or every managed project.
///
/// Writes do not leave backup files. A mutating run requires `--force` or interactive
/// confirmation; non-interactive runs without `--force` fail closed.
pub fn run_clean(
    cfg: &Config,
    only_project: Option<&str>,
    date: &str,
    dry_run: bool,
    force: bool,
    confirmation: &impl Fn(&str, DefaultAnswer) -> Confirmation,
) -> Result<String, String> {
    run_clean_typed(cfg, only_project, date, dry_run, force, confirmation)
        .map_err(|error| error.to_string())
}

pub(crate) fn run_clean_typed(
    cfg: &Config,
    only_project: Option<&str>,
    date: &str,
    dry_run: bool,
    force: bool,
    confirmation: &impl Fn(&str, DefaultAnswer) -> Confirmation,
) -> Result<String, CleanError> {
    if !Path::new(&cfg.notes_dir).exists() {
        return Err(CleanError::NotesDirectoryNotFound {
            path: cfg.notes_dir.clone(),
        });
    }
    let projects: Vec<String> = if let Some(p) = only_project {
        vec![p.to_string()]
    } else {
        let mut v: Vec<String> = cfg.projects.keys().cloned().collect();
        v.sort();
        v
    };

    let mut plans: Vec<ProjectPlan> = Vec::new();
    for project in &projects {
        let index = project_index_path(cfg.notes_dir_for(project), project);
        if !index.exists() {
            continue;
        }
        plans.push(plan_project(project, &index, date)?);
    }

    let cleanable = plans
        .iter()
        .flat_map(|p| &p.results)
        .filter(|r| r.status != "skipped")
        .count();

    if !dry_run && cleanable > 0 {
        let apply = if force {
            true
        } else {
            let question = format!(
                "{}Clean {cleanable} done work-item(s)? Edits notes-pro (git-tracked; no .bak)",
                render_text(&plans, true)
            );
            match confirmation(&question, DefaultAnswer::No) {
                Confirmation::Accepted => true,
                Confirmation::Declined => false,
                Confirmation::NonInteractive => return Err(CleanError::NoTtyToConfirm),
            }
        };
        if !apply {
            return Ok("Aborted; nothing changed.\n".to_string());
        }
        for p in &mut plans {
            for w in &p.writes {
                fs_atomic::write_text_atomic(&w.path, &w.content).map_err(|source| {
                    CleanError::Write {
                        path: w.path.clone(),
                        source,
                    }
                })?;
            }
            for r in &mut p.results {
                if r.status == "wouldClean" {
                    r.status = "cleaned".to_string();
                }
            }
        }
    }

    let mut out = render_text(&plans, dry_run);
    if out.is_empty() {
        let target =
            only_project.map_or_else(|| cfg.notes_dir.clone(), std::string::ToString::to_string);
        let _ = writeln!(out, "No done work-item links to clean in {target}.");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use std::{assert_matches, error::Error};

    use super::*;
    use crate::config;

    fn declined(_: &str, _: DefaultAnswer) -> Confirmation {
        Confirmation::Declined
    }

    fn noninteractive(_: &str, _: DefaultAnswer) -> Confirmation {
        Confirmation::NonInteractive
    }

    fn stage_clean_fixture() -> (tempfile::TempDir, config::Config) {
        let stage = tempfile::tempdir().unwrap();
        let notes = stage.path().join("notes");
        let proj = notes.join("cfg");
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::write(
            proj.join("cfg.md"),
            "- [x] [[CFG-0001|test item]] ✅ 2026-06-01\n",
        )
        .unwrap();
        std::fs::write(
            proj.join("CFG-0001.md"),
            "---\nstatus: active\ntitle: test item\nproject: cfg\ncreated: 2026-01-01\n---\n\nbody\n",
        )
        .unwrap();
        let cfg = config::from_json(
            &format!(
                r#"{{ "notesDir": "{}", "projects": {{ "cfg": "/repo" }}, "prefixes": {{ "cfg": "CFG" }} }}"#,
                notes.to_string_lossy().replace('\\', "\\\\")
            ),
            None,
        )
        .unwrap();
        (stage, cfg)
    }

    #[test]
    fn noninteractive_clean_without_force_errors() {
        let (_stage, cfg) = stage_clean_fixture();
        let err =
            run_clean_typed(&cfg, None, "2026-06-20", false, false, &noninteractive).unwrap_err();
        assert_matches!(err, CleanError::NoTtyToConfirm);
    }

    #[test]
    fn force_applies_and_returns_text_summary() {
        let (_stage, cfg) = stage_clean_fixture();
        let out = run_clean_typed(
            &cfg,
            None,
            "2026-06-20",
            false,
            true, // force
            &declined,
        )
        .unwrap();
        assert!(
            out.contains("CLEANED"),
            "expected CLEANED in output, got: {out}"
        );
    }

    #[test]
    fn finds_checked_wikilinks_only() {
        let content = "\
- [x] [[CFG-0012|obsidian]] ✅ 2026-06-05
- [X] [[CFG-0010|trim skills]]
- [ ] [[CFG-0015|open item]]
- [[CFG-0001|bare link]]
- [x] plain checkbox no wikilink
";
        let links = find_done_index_links(content);
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].id, "CFG-0012");
        assert_eq!(links[0].completed.as_deref(), Some("2026-06-05"));
        assert_eq!(links[1].id, "CFG-0010");
        assert_eq!(links[1].completed, None);
    }

    #[test]
    fn completed_precedence_checkmark_then_fm_then_fallback() {
        assert_eq!(
            resolve_completed(Some("2026-06-05"), Some("2026-02-02"), "2026-01-01"),
            "2026-06-05"
        );
        assert_eq!(
            resolve_completed(None, Some("2026-02-02"), "2026-01-01"),
            "2026-02-02"
        );
        assert_eq!(resolve_completed(None, None, "2026-01-01"), "2026-01-01");
        assert_eq!(
            resolve_completed(Some("  "), Some("2026-02-02"), "2026-01-01"),
            "2026-02-02"
        );
    }

    #[test]
    fn clean_error_preserves_legacy_display_text() {
        let err = CleanError::NotesDirectoryNotFound {
            path: "/tmp/missing-notes".to_string(),
        };
        assert_matches!(err, CleanError::NotesDirectoryNotFound { .. });
        assert_eq!(
            err.to_string(),
            "Notes directory not found: /tmp/missing-notes"
        );

        let err = CleanError::NoTtyToConfirm;
        assert_matches!(err, CleanError::NoTtyToConfirm);
        assert_eq!(
            err.to_string(),
            "No TTY to confirm a clean. Re-run with --dry-run to preview or --force to apply."
        );
    }

    #[test]
    fn clean_io_error_variants_preserve_source() {
        let source = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "nope");
        let err = CleanError::Write {
            path: PathBuf::from("/tmp/item.md"),
            source,
        };

        assert_matches!(err, CleanError::Write { .. });
        assert_eq!(err.to_string(), "Cannot write /tmp/item.md: nope");
        assert_eq!(err.source().unwrap().to_string(), "nope");
    }
}
