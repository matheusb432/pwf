//! Project notes indexed under a `### Notes` section, separate from pending-work items.

pub mod errors;
pub mod id;
pub mod index;
pub mod render;
pub mod store;

use std::path::{Path, PathBuf};

pub use errors::NoteError;
pub use id::NoteId;
use pwf_domain::project::{ProjectName, ProjectPrefix};

const DEFAULT_LS_CAP: usize = 10;

/// Contains a note operation and its payload.
#[derive(Debug, Clone)]
pub enum NoteVerb {
    Ls { number: Option<usize> },
    Add { message: String },
    Remove { id: String },
    Update { id: String, message: String },
}

/// Contains a parsed `pwf note` invocation.
#[derive(Debug, Clone)]
pub struct NoteCommand {
    pub target: NoteTarget,
    pub verb: NoteVerb,
    pub date: Option<String>,
}

/// Identifies the project files used by a note operation.
#[derive(Debug, Clone)]
pub struct NoteTarget {
    pub project: ProjectName,
    pub prefix: ProjectPrefix,
    pub tasks_path: PathBuf,
}

/// Executes a note command and returns Markdown output.
///
/// Mutating commands write files before returning.
///
/// # Errors
///
/// Returns a rendered note-operation error.
pub fn run(command: &NoteCommand) -> Result<String, String> {
    let target = &command.target;
    let project_dir = &target.tasks_path;
    let index_path = target.tasks_path.join(format!("{}.md", target.project));

    match &command.verb {
        NoteVerb::Ls { number } => Ok(render_ls(
            project_dir,
            &target.project,
            &target.prefix,
            *number,
        )),
        NoteVerb::Add { message } => add_note(
            project_dir,
            &index_path,
            &target.project,
            &target.prefix,
            message,
            command.date.as_deref(),
        ),
        NoteVerb::Remove { id } => remove_note(project_dir, &index_path, &target.prefix, id),
        NoteVerb::Update { id, message } => update_note(project_dir, &target.prefix, id, message),
    }
}

fn render_ls(
    project_dir: &Path,
    project: &ProjectName,
    prefix: &ProjectPrefix,
    number: Option<usize>,
) -> String {
    let notes = store::list(project_dir, prefix.as_ref());
    if notes.is_empty() {
        return format!("No notes for {project}.\n");
    }
    let cap = number.unwrap_or(DEFAULT_LS_CAP);
    render::render_list(&notes, cap)
}

fn add_note(
    project_dir: &Path,
    index_path: &Path,
    project: &ProjectName,
    prefix: &ProjectPrefix,
    message: &str,
    date: Option<&str>,
) -> Result<String, String> {
    if message.trim().is_empty() {
        return Err(NoteError::EmptyMessage.into());
    }
    let id = store::allocate_id(project_dir, prefix.as_ref());
    let created = pwf_core::date::stamp_date(date);
    store::create(project_dir, &id, project.as_ref(), message, &created)?;
    let existing = std::fs::read_to_string(index_path).unwrap_or_default();
    let updated = index::add_note_link(&existing, &id.canonical);
    pwf_core::fs_atomic::write_text_atomic(index_path, &updated).map_err(|source| {
        NoteError::Io {
            context: "Cannot write index".to_string(),
            source,
        }
    })?;
    Ok(format!("Added {} :: {}\n", id.canonical, message.trim()))
}

fn remove_note(
    project_dir: &Path,
    index_path: &Path,
    prefix: &ProjectPrefix,
    raw_id: &str,
) -> Result<String, String> {
    let id = NoteId::resolve(raw_id, prefix.as_ref())?;
    store::delete(project_dir, &id)?; // errors NoSuchNote before touching the index
    let existing = std::fs::read_to_string(index_path).unwrap_or_default();
    let updated = index::remove_note_link(&existing, &id.canonical);
    pwf_core::fs_atomic::write_text_atomic(index_path, &updated).map_err(|source| {
        NoteError::Io {
            context: "Cannot write index".to_string(),
            source,
        }
    })?;
    Ok(format!("Removed {}\n", id.canonical))
}

fn update_note(
    project_dir: &Path,
    prefix: &ProjectPrefix,
    raw_id: &str,
    message: &str,
) -> Result<String, String> {
    if message.trim().is_empty() {
        return Err(NoteError::EmptyMessage.into());
    }
    let id = NoteId::resolve(raw_id, prefix.as_ref())?;
    store::update(project_dir, &id, message)?;
    Ok(format!("Updated {} :: {}\n", id.canonical, message.trim()))
}

#[cfg(test)]
mod run_tests {
    use std::path::Path;

    use pwf_domain::project::{ProjectName, ProjectPrefix};

    use super::*;

    fn command(root: &Path, verb: NoteVerb) -> NoteCommand {
        NoteCommand {
            target: NoteTarget {
                project: ProjectName::try_new("pwf").unwrap(),
                prefix: ProjectPrefix::try_new("PWF").unwrap(),
                tasks_path: root.join("tasks"),
            },
            verb,
            date: Some("2026-06-28".to_string()),
        }
    }

    #[test]
    fn add_list_remove_round_trip() {
        let root = tempfile::tempdir().unwrap();
        let out = run(&command(
            root.path(),
            NoteVerb::Add {
                message: "buy milk".into(),
            },
        ))
        .unwrap();
        assert!(out.contains("PWF-NOTE-0001 :: buy milk"), "got: {out}");

        let listed = run(&command(root.path(), NoteVerb::Ls { number: None })).unwrap();
        assert!(
            listed.contains("PWF-NOTE-0001 :: buy milk"),
            "got: {listed}"
        );

        let removed = run(&command(root.path(), NoteVerb::Remove { id: "1".into() })).unwrap();
        assert!(removed.contains("Removed PWF-NOTE-0001"), "got: {removed}");

        let empty = run(&command(root.path(), NoteVerb::Ls { number: None })).unwrap();
        assert!(empty.contains("No notes"), "got: {empty}");
    }

    #[test]
    fn add_rejects_empty_message() {
        let root = tempfile::tempdir().unwrap();
        let note_path = root.path().join("tasks").join("PWF-NOTE-0001.md");
        let index_path = root.path().join("tasks").join("pwf.md");

        for bad_msg in ["", "   "] {
            let err = run(&command(
                root.path(),
                NoteVerb::Add {
                    message: bad_msg.into(),
                },
            ))
            .unwrap_err();
            assert!(
                err.contains("empty"),
                "expected 'empty' in error for {bad_msg:?}, got: {err}"
            );
            assert!(
                !note_path.exists(),
                "note file must not be created for msg {bad_msg:?}"
            );
            assert!(
                !index_path.exists()
                    || !std::fs::read_to_string(&index_path)
                        .unwrap_or_default()
                        .contains("PWF-NOTE"),
                "index must not gain a link for msg {bad_msg:?}"
            );
        }
    }

    #[test]
    fn add_writes_project_name_not_prefix_in_frontmatter() {
        let root = tempfile::tempdir().unwrap();
        run(&command(
            root.path(),
            NoteVerb::Add {
                message: "frontmatter check".into(),
            },
        ))
        .unwrap();
        let note_path = root.path().join("tasks").join("PWF-NOTE-0001.md");
        let content = std::fs::read_to_string(&note_path)
            .unwrap_or_else(|e| panic!("could not read {}: {e}", note_path.display()));
        assert!(
            content.contains("project: pwf"),
            "expected 'project: pwf' in:\n{content}"
        );
        assert!(
            !content.contains("project: PWF"),
            "unexpected 'project: PWF' in:\n{content}"
        );
    }

    #[test]
    fn update_replaces_message_without_changing_index() {
        let root = tempfile::tempdir().unwrap();
        run(&command(
            root.path(),
            NoteVerb::Add {
                message: "old note".into(),
            },
        ))
        .unwrap();
        let index_path = root.path().join("tasks").join("pwf.md");
        let before_index = std::fs::read_to_string(&index_path).unwrap();

        let out = run(&command(
            root.path(),
            NoteVerb::Update {
                id: "1".into(),
                message: "new note".into(),
            },
        ))
        .unwrap();

        assert!(
            out.contains("Updated PWF-NOTE-0001 :: new note"),
            "got: {out}"
        );
        assert_eq!(std::fs::read_to_string(&index_path).unwrap(), before_index);
        let listed = run(&command(root.path(), NoteVerb::Ls { number: None })).unwrap();
        assert!(
            listed.contains("PWF-NOTE-0001 :: new note"),
            "got: {listed}"
        );
        assert!(!listed.contains("old note"), "old note retained: {listed}");
    }

    #[test]
    fn update_rejects_empty_message_without_changing_note() {
        let root = tempfile::tempdir().unwrap();
        run(&command(
            root.path(),
            NoteVerb::Add {
                message: "old note".into(),
            },
        ))
        .unwrap();
        let note_path = root.path().join("tasks").join("PWF-NOTE-0001.md");
        let before = std::fs::read_to_string(&note_path).unwrap();

        let err = run(&command(
            root.path(),
            NoteVerb::Update {
                id: "1".into(),
                message: "   ".into(),
            },
        ))
        .unwrap_err();

        assert!(err.contains("empty"), "got: {err}");
        assert_eq!(std::fs::read_to_string(&note_path).unwrap(), before);
    }
}
