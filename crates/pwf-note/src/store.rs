//! Creates, lists, updates, and deletes note files.
//!
//! Bodies store the trimmed message; frontmatter records `type: note`.

use std::{fmt::Write, path::Path};

use regex::Regex;

use crate::{errors::NoteError, id::NoteId};

/// Contains the note fields needed for list rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub id: String,
    pub number: u32,
    pub message: String,
}

/// Allocates the next `{prefix}-NOTE-NNNN` id by scanning `project_dir`.
///
/// # Panics
///
/// Panics if [`pwf_core::id::next_id`] returns a non-canonical id.
pub fn allocate_id(project_dir: &Path, prefix: &str) -> NoteId {
    let key = format!("{prefix}-NOTE");
    let raw = pwf_core::id::next_id(&[project_dir], &key);
    NoteId::resolve(&raw, prefix).expect("next_id yields a canonical note id")
}

/// Writes note frontmatter and body atomically.
///
/// # Errors
///
/// Returns [`NoteError::Io`] when the note cannot be written.
pub fn create(
    project_dir: &Path,
    id: &NoteId,
    project: &str,
    message: &str,
    created: &str,
) -> Result<(), NoteError> {
    let content = note_content(project, created, message);
    let path = project_dir.join(id.file_name());
    pwf_core::fs_atomic::write_text_atomic(&path, &content).map_err(|source| NoteError::Io {
        context: format!("Cannot write note {}", id.canonical),
        source,
    })
}

/// Replaces the note body while preserving frontmatter.
///
/// # Errors
///
/// Returns [`NoteError::NoSuchNote`] when the note is absent or [`NoteError::Io`] on read or write
/// failure.
pub fn update(project_dir: &Path, id: &NoteId, message: &str) -> Result<(), NoteError> {
    let path = project_dir.join(id.file_name());
    if !path.exists() {
        return Err(NoteError::NoSuchNote {
            id: id.canonical.clone(),
            project: project_dir
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string(),
        });
    }
    let raw = std::fs::read_to_string(&path).map_err(|source| NoteError::Io {
        context: format!("Cannot read note {}", id.canonical),
        source,
    })?;
    let content = replace_body(&raw, message);
    pwf_core::fs_atomic::write_text_atomic(&path, &content).map_err(|source| NoteError::Io {
        context: format!("Cannot write note {}", id.canonical),
        source,
    })
}

fn note_content(project: &str, created: &str, message: &str) -> String {
    let mut out = String::new();
    out.push_str("---\n");
    out.push_str("type: note\n");
    let _ = writeln!(out, "project: {project}");
    let _ = writeln!(out, "created: {created}");
    out.push_str("---\n\n");
    out.push_str(message.trim());
    out.push('\n');
    out
}

fn replace_body(raw: &str, message: &str) -> String {
    let Some((frontmatter, _body)) = raw.split_once("\n\n") else {
        return format!("{}\n", message.trim());
    };
    format!("{frontmatter}\n\n{}\n", message.trim())
}

/// Lists notes by descending numeric suffix.
///
/// # Panics
///
/// Panics if the escaped prefix cannot compile as a regular expression.
pub fn list(project_dir: &Path, prefix: &str) -> Vec<Note> {
    let re = Regex::new(&format!(r"^{}-NOTE-(\d{{4}})$", regex::escape(prefix))).unwrap();
    let mut notes: Vec<Note> = Vec::new();
    for entry in std::fs::read_dir(project_dir)
        .into_iter()
        .flatten()
        .flatten()
    {
        let p = entry.path();
        if p.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let Some(stem) = p.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let Some(c) = re.captures(stem) else { continue };
        let number: u32 = c[1].parse().unwrap_or(0);
        let text = std::fs::read_to_string(&p).unwrap_or_default();
        notes.push(Note {
            id: stem.to_string(),
            number,
            message: message_of(&text),
        });
    }
    notes.sort_by_key(|n| std::cmp::Reverse(n.number));
    notes
}

/// Returns the first non-empty body line after frontmatter.
fn message_of(text: &str) -> String {
    let body = pwf_core::frontmatter::parse(text).body;
    body.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .to_string()
}

/// Deletes a note file.
///
/// # Errors
///
/// Returns [`NoteError::NoSuchNote`] when the note is absent or [`NoteError::Io`] when removal
/// fails.
pub fn delete(project_dir: &Path, id: &NoteId) -> Result<(), NoteError> {
    let path = project_dir.join(id.file_name());
    if !path.exists() {
        return Err(NoteError::NoSuchNote {
            id: id.canonical.clone(),
            project: project_dir
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string(),
        });
    }
    std::fs::remove_file(&path).map_err(|source| NoteError::Io {
        context: format!("Cannot remove note {}", id.canonical),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn allocate_ignores_task_files() {
        let dir = tempdir();
        let d = dir.path();
        std::fs::write(d.join("PWF-0007.md"), "task").unwrap();
        assert_eq!(allocate_id(d, "PWF").canonical, "PWF-NOTE-0001");
    }

    #[test]
    fn create_then_list_round_trips_message_newest_first() {
        let dir = tempdir();
        let d = dir.path();
        let one = NoteId::resolve("1", "PWF").unwrap();
        let two = NoteId::resolve("2", "PWF").unwrap();
        create(d, &one, "pwf", "first note", "2026-06-28").unwrap();
        create(d, &two, "pwf", "second note", "2026-06-28").unwrap();
        let notes = list(d, "PWF");
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[0].id, "PWF-NOTE-0002");
        assert_eq!(notes[0].message, "second note");
        assert_eq!(notes[1].message, "first note");
        let raw = std::fs::read_to_string(d.join("PWF-NOTE-0001.md")).unwrap();
        assert!(raw.contains("type: note"), "got: {raw}");
        assert!(
            !raw.contains("status:"),
            "note must not look like a task: {raw}"
        );
    }

    #[test]
    fn delete_removes_file_and_errors_when_absent() {
        let dir = tempdir();
        let d = dir.path();
        let id = NoteId::resolve("1", "PWF").unwrap();
        create(d, &id, "pwf", "x", "2026-06-28").unwrap();
        delete(d, &id).unwrap();
        assert!(!d.join("PWF-NOTE-0001.md").exists());
        assert!(matches!(delete(d, &id), Err(NoteError::NoSuchNote { .. })));
    }

    #[test]
    fn update_replaces_message_and_preserves_frontmatter() {
        let dir = tempdir();
        let d = dir.path();
        let id = NoteId::resolve("1", "PWF").unwrap();
        create(d, &id, "pwf", "old message", "2026-06-28").unwrap();

        update(d, &id, "new message").unwrap();

        let raw = std::fs::read_to_string(d.join("PWF-NOTE-0001.md")).unwrap();
        assert!(raw.contains("type: note"), "frontmatter lost: {raw}");
        assert!(raw.contains("project: pwf"), "project lost: {raw}");
        assert!(raw.contains("created: 2026-06-28"), "created lost: {raw}");
        assert!(
            raw.ends_with("\n\nnew message\n"),
            "message not replaced: {raw}"
        );
        assert!(!raw.contains("old message"), "old message retained: {raw}");
    }

    #[test]
    fn update_errors_when_note_is_absent() {
        let dir = tempdir();
        let id = NoteId::resolve("1", "PWF").unwrap();

        assert!(matches!(
            update(dir.path(), &id, "new message"),
            Err(NoteError::NoSuchNote { .. })
        ));
    }
}
