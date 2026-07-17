//! Project notes indexed under a `### Notes` section, separate from pending-work items.

pub mod errors;
pub mod id;
pub mod index;
pub mod render;
pub mod store;

use std::path::Path;

pub use errors::NoteError;
pub use id::NoteId;

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
    pub project: String,
    pub verb: NoteVerb,
    pub config_path: Option<String>,
    pub notes_dir: Option<String>,
    pub date: Option<String>,
}

/// Executes a note command and returns Markdown output.
///
/// Mutating commands write files before returning.
///
/// # Errors
///
/// Returns a rendered configuration or note-operation error.
pub fn run(command: &NoteCommand) -> Result<String, String> {
    let cfg = load_config(command)?;
    let prefix = pwf_core::paths::project_key(&cfg, &command.project)
        .ok_or_else(|| NoteError::UnknownProject(command.project.clone()))?
        .to_string();
    let notes_dir = cfg.notes_dir_for(&command.project);
    let project_dir = pwf_core::paths::project_dir(notes_dir, &command.project);
    let index_path = pwf_core::paths::project_index_path(notes_dir, &command.project);

    match &command.verb {
        NoteVerb::Ls { number } => Ok(render_ls(&project_dir, &command.project, &prefix, *number)),
        NoteVerb::Add { message } => add_note(
            &project_dir,
            &index_path,
            &command.project,
            &prefix,
            message,
            command.date.as_deref(),
        ),
        NoteVerb::Remove { id } => remove_note(&project_dir, &index_path, &prefix, id),
        NoteVerb::Update { id, message } => update_note(&project_dir, &prefix, id, message),
    }
}

fn load_config(command: &NoteCommand) -> Result<pwf_core::config::Config, String> {
    let path = command
        .config_path
        .clone()
        .or_else(pwf_core::config::default_config_path)
        .ok_or_else(|| "could not resolve pwf config path".to_string())?;
    pwf_core::config::load(&path, command.notes_dir.as_deref()).map_err(String::from)
}

fn render_ls(project_dir: &Path, project: &str, prefix: &str, number: Option<usize>) -> String {
    let notes = store::list(project_dir, prefix);
    if notes.is_empty() {
        return format!("No notes for {project}.\n");
    }
    let cap = number.unwrap_or(DEFAULT_LS_CAP);
    render::render_list(&notes, cap)
}

fn add_note(
    project_dir: &Path,
    index_path: &Path,
    project: &str,
    prefix: &str,
    message: &str,
    date: Option<&str>,
) -> Result<String, String> {
    if message.trim().is_empty() {
        return Err(NoteError::EmptyMessage.into());
    }
    let id = store::allocate_id(project_dir, prefix);
    let created = pwf_core::date::stamp_date(date);
    store::create(project_dir, &id, project, message, &created)?;
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
    prefix: &str,
    raw_id: &str,
) -> Result<String, String> {
    let id = NoteId::resolve(raw_id, prefix)?;
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
    prefix: &str,
    raw_id: &str,
    message: &str,
) -> Result<String, String> {
    if message.trim().is_empty() {
        return Err(NoteError::EmptyMessage.into());
    }
    let id = NoteId::resolve(raw_id, prefix)?;
    store::update(project_dir, &id, message)?;
    Ok(format!("Updated {} :: {}\n", id.canonical, message.trim()))
}

#[cfg(test)]
mod run_tests {
    use super::*;

    fn sandbox() -> (tempfile::TempDir, String) {
        let root = tempfile::tempdir().unwrap();
        let cfg = root.path().join("pending-work.json");
        std::fs::write(
            &cfg,
            format!(
                r#"{{ "notesDir": "{}", "projects": {{ "pwf": "/repo" }}, "prefixes": {{ "pwf": "PWF" }} }}"#,
                root.path().join("notes").to_string_lossy().replace('\\', "/")
            ),
        )
        .unwrap();
        let cfg = cfg.to_string_lossy().into_owned();
        (root, cfg)
    }

    fn cmd(project: &str, verb: NoteVerb, config_path: &str) -> NoteCommand {
        NoteCommand {
            project: project.to_string(),
            verb,
            config_path: Some(config_path.to_string()),
            notes_dir: None,
            date: Some("2026-06-28".to_string()),
        }
    }

    #[test]
    fn add_list_remove_round_trip() {
        let (_root, cfg) = sandbox();
        let out = run(&cmd(
            "pwf",
            NoteVerb::Add {
                message: "buy milk".into(),
            },
            &cfg,
        ))
        .unwrap();
        assert!(out.contains("PWF-NOTE-0001 :: buy milk"), "got: {out}");

        let listed = run(&cmd("pwf", NoteVerb::Ls { number: None }, &cfg)).unwrap();
        assert!(
            listed.contains("PWF-NOTE-0001 :: buy milk"),
            "got: {listed}"
        );

        let removed = run(&cmd("pwf", NoteVerb::Remove { id: "1".into() }, &cfg)).unwrap();
        assert!(removed.contains("Removed PWF-NOTE-0001"), "got: {removed}");

        let empty = run(&cmd("pwf", NoteVerb::Ls { number: None }, &cfg)).unwrap();
        assert!(empty.contains("No notes"), "got: {empty}");
    }

    #[test]
    fn unknown_project_errors() {
        let (_root, cfg) = sandbox();
        let err = run(&cmd("nope", NoteVerb::Ls { number: None }, &cfg)).unwrap_err();
        assert!(err.contains("no configured prefix"), "got: {err}");
    }

    #[test]
    fn add_rejects_empty_message() {
        let (root, cfg) = sandbox();
        let notes_dir = root.path().join("notes");
        let note_path = notes_dir.join("pwf").join("PWF-NOTE-0001.md");
        let index_path = notes_dir.join("pwf").join("index.md");

        for bad_msg in ["", "   "] {
            let err = run(&cmd(
                "pwf",
                NoteVerb::Add {
                    message: bad_msg.into(),
                },
                &cfg,
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
        let (root, cfg) = sandbox();
        run(&cmd(
            "pwf",
            NoteVerb::Add {
                message: "frontmatter check".into(),
            },
            &cfg,
        ))
        .unwrap();
        let note_path = root
            .path()
            .join("notes")
            .join("pwf")
            .join("PWF-NOTE-0001.md");
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
        let (root, cfg) = sandbox();
        run(&cmd(
            "pwf",
            NoteVerb::Add {
                message: "old note".into(),
            },
            &cfg,
        ))
        .unwrap();
        let index_path = root.path().join("notes").join("pwf").join("pwf.md");
        let before_index = std::fs::read_to_string(&index_path).unwrap();

        let out = run(&cmd(
            "pwf",
            NoteVerb::Update {
                id: "1".into(),
                message: "new note".into(),
            },
            &cfg,
        ))
        .unwrap();

        assert!(
            out.contains("Updated PWF-NOTE-0001 :: new note"),
            "got: {out}"
        );
        assert_eq!(std::fs::read_to_string(&index_path).unwrap(), before_index);
        let listed = run(&cmd("pwf", NoteVerb::Ls { number: None }, &cfg)).unwrap();
        assert!(
            listed.contains("PWF-NOTE-0001 :: new note"),
            "got: {listed}"
        );
        assert!(!listed.contains("old note"), "old note retained: {listed}");
    }

    #[test]
    fn update_rejects_empty_message_without_changing_note() {
        let (root, cfg) = sandbox();
        run(&cmd(
            "pwf",
            NoteVerb::Add {
                message: "old note".into(),
            },
            &cfg,
        ))
        .unwrap();
        let note_path = root
            .path()
            .join("notes")
            .join("pwf")
            .join("PWF-NOTE-0001.md");
        let before = std::fs::read_to_string(&note_path).unwrap();

        let err = run(&cmd(
            "pwf",
            NoteVerb::Update {
                id: "1".into(),
                message: "   ".into(),
            },
            &cfg,
        ))
        .unwrap_err();

        assert!(err.contains("empty"), "got: {err}");
        assert_eq!(std::fs::read_to_string(&note_path).unwrap(), before);
    }
}
