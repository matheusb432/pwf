//! `clean` action: sweep each project index for done `- [x] [[KEY-NNNN|…]]`
//! links, stamp the backing file done+completed, and remove the link.
//! File-model analogue of scripts/notes-todo-cleaner (the legacy checkbox model).

use std::path::{Path, PathBuf};

use regex::Regex;

use super::pending_work::{project_index_path, set_status_text};
use crate::{
    config::Config,
    confirm::{Confirm, DefaultAnswer},
    frontmatter, fs_atomic,
};

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

/// One `- [x] [[KEY-NNNN|…]]` line located in an index note. The span covers the
/// whole line plus its trailing newline so it can be excised cleanly.
#[derive(Debug, Clone, PartialEq)]
pub struct DoneLink {
    pub id: String,
    pub completed: Option<String>,
    pub start: usize,
    pub end: usize,
}

/// Find checked work-item links. Ignores open (`- [ ]`), bare (`- [[…]]`), and
/// plain checkbox lines without a wikilink.
pub fn find_done_index_links(content: &str) -> Vec<DoneLink> {
    let re = Regex::new(
        r"(?m)^[ \t]*-[ \t]*\[[xX]\][ \t]*\[\[(?P<id>[A-Z]{2,4}-\d{4})(?:\|[^\]]*)?\]\][^\r\n]*\r?\n?",
    )
    .unwrap();
    let date_re = Regex::new(r"✅\s*(\d{4}-\d{2}-\d{2})").unwrap();
    re.captures_iter(content)
        .map(|m| {
            let whole = m.get(0).unwrap();
            let completed = date_re.captures(whole.as_str()).map(|c| c[1].to_string());
            DoneLink {
                id: m["id"].to_string(),
                completed,
                start: whole.start(),
                end: whole.end(),
            }
        })
        .collect()
}

/// Completed-date precedence: ✅ stamp on the index line → existing frontmatter
/// `completed:` → caller fallback (`--date`/today). Empty/whitespace values skip.
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

/// Per-item outcome of a clean sweep.
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

/// Remove non-overlapping, ascending byte spans from `content`.
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

/// A file write the cleaner intends to perform (computed before any I/O).
struct PendingWrite {
    path: PathBuf,
    content: String,
}

/// What a clean would do for one project: the report rows plus the writes.
struct ProjectPlan {
    project: String,
    results: Vec<CleanResult>,
    writes: Vec<PendingWrite>,
}

/// Compute (without writing) what a clean would do for one project.
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
        // Stamp the item file first, unlink the index last: on a mid-apply failure
        // a link is only ever removed after its backing file is stamped done.
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

/// Render the per-project text summary. `dry_run` only flips the header verb.
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
        out.push_str(&format!(
            "{verb} {}: {cleaned} {label}, {skipped} skipped\n",
            p.project
        ));
        for r in &p.results {
            if r.status == "skipped" {
                out.push_str(&format!(
                    "  {} skipped ({})\n",
                    r.id,
                    r.issue.as_deref().unwrap_or("skipped")
                ));
            } else {
                out.push_str(&format!(
                    "  {} done (completed {})\n",
                    r.id,
                    r.completed.as_deref().unwrap_or("")
                ));
            }
        }
    }
    out
}

/// `pw clean`: sweep one project (`only_project`, already resolved) or all
/// managed projects.
///
/// notes-pro is git-tracked, so writes do NOT leave `.bak` files. A real
/// (non-dry-run) clean is gated: `--dry-run` previews; `--force` applies
/// without asking; an interactive run prints the plan and asks to confirm; a
/// non-interactive run without `--force` refuses rather than mutate silently.
pub fn run_clean(
    cfg: &Config,
    only_project: Option<&str>,
    date: &str,
    dry_run: bool,
    force: bool,
    confirmer: &dyn Confirm,
) -> Result<String, String> {
    run_clean_typed(cfg, only_project, date, dry_run, force, confirmer)
        .map_err(|error| error.to_string())
}

pub(crate) fn run_clean_typed(
    cfg: &Config,
    only_project: Option<&str>,
    date: &str,
    dry_run: bool,
    force: bool,
    confirmer: &dyn Confirm,
) -> Result<String, CleanError> {
    if !Path::new(&cfg.notes_dir).exists() {
        return Err(CleanError::NotesDirectoryNotFound {
            path: cfg.notes_dir.clone(),
        });
    }
    let projects: Vec<String> = match only_project {
        Some(p) => vec![p.to_string()],
        None => {
            let mut v: Vec<String> = cfg.projects.keys().cloned().collect();
            v.sort();
            v
        }
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

    // Confirmation gate — only when a real run would actually mutate something.
    if !dry_run && cleanable > 0 {
        let apply = if force {
            true
        } else if confirmer.interactive() {
            eprint!("{}", render_text(&plans, true));
            confirmer.confirm(
                &format!(
                    "Clean {cleanable} done work-item(s)? Edits notes-pro (git-tracked; no .bak)"
                ),
                DefaultAnswer::No,
            )
        } else {
            return Err(CleanError::NoTtyToConfirm);
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
        let target = only_project
            .map(|p| p.to_string())
            .unwrap_or_else(|| cfg.notes_dir.clone());
        out.push_str(&format!("No done work-item links to clean in {target}.\n"));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use super::*;
    use crate::{config, confirm::FakeConfirm};

    fn nanos() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos() as u128
    }

    /// Build a minimal Config pointing at a temp notes dir with one done link.
    fn stage_clean_fixture() -> (std::path::PathBuf, config::Config) {
        let stage = std::env::temp_dir().join(format!("pwf_clean_{}", nanos()));
        let notes = stage.join("notes");
        let proj = notes.join("cfg");
        std::fs::create_dir_all(&proj).unwrap();
        // Index has one checked wikilink.
        std::fs::write(
            proj.join("cfg.md"),
            "- [x] [[CFG-0001|test item]] ✅ 2026-06-01\n",
        )
        .unwrap();
        // Backing item file.
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
        // Non-interactive + no --force must refuse rather than mutate silently.
        let (_stage, cfg) = stage_clean_fixture();
        let err = run_clean_typed(
            &cfg,
            None,
            "2026-06-20",
            false,
            false,
            &FakeConfirm {
                interactive: false,
                answer: false,
            },
        )
        .unwrap_err();
        assert!(matches!(err, CleanError::NoTtyToConfirm));
    }

    #[test]
    fn force_applies_and_returns_text_summary() {
        // --force bypasses the confirm gate and returns the CLEANED text line.
        let (_stage, cfg) = stage_clean_fixture();
        let out = run_clean_typed(
            &cfg,
            None,
            "2026-06-20",
            false,
            true, // force
            &FakeConfirm {
                interactive: false,
                answer: false,
            },
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
        // empty ✅ is ignored, falls through to frontmatter
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
        assert!(matches!(err, CleanError::NotesDirectoryNotFound { .. }));
        assert_eq!(
            err.to_string(),
            "Notes directory not found: /tmp/missing-notes"
        );

        let err = CleanError::NoTtyToConfirm;
        assert!(matches!(err, CleanError::NoTtyToConfirm));
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

        assert!(matches!(err, CleanError::Write { .. }));
        assert_eq!(err.to_string(), "Cannot write /tmp/item.md: nope");
        assert_eq!(err.source().unwrap().to_string(), "nope");
    }
}
