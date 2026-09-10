//! Edits selected fields of one note in a managed project.

use pwf_models::{
    note::NoteSelector,
    project::{ProjectId, ProjectName},
};
use pwf_wire::note::{EditNote, MutatedNote};

use crate::{
    ports::project_note::{ProjectNotePatch, ProjectNotes},
    project::{get_active_project, get_project::GetProjectError},
};

#[derive(Debug, thiserror::Error)]
pub enum EditNoteError {
    #[error(transparent)]
    GetProject(#[from] GetProjectError),
    #[error("Note id '{selector}' does not belong to project {project_id}.")]
    ProjectMismatch {
        selector: NoteSelector,
        project_id: ProjectId,
    },
    #[error("No such note {id} in {project}.")]
    NoSuchNote {
        id: pwf_models::note::NoteId,
        project: ProjectName,
    },
    #[error(transparent)]
    Store(anyhow::Error),
}

/// Resolves one note and applies only the explicitly selected changes.
#[cqrsy::command]
pub async fn execute(
    command: EditNote,
    store: &impl ProjectNotes,
    pool: &sqlx::SqlitePool,
) -> Result<MutatedNote, EditNoteError> {
    let project = get_active_project::execute(command.project_id, pool).await?;
    let id =
        command
            .selector
            .resolve(&project.id)
            .ok_or_else(|| EditNoteError::ProjectMismatch {
                selector: command.selector,
                project_id: project.id.clone(),
            })?;
    let existing = store
        .get_note(&project, &id)
        .map_err(|error| EditNoteError::Store(anyhow::Error::new(error)))?
        .ok_or_else(|| EditNoteError::NoSuchNote {
            id: id.clone(),
            project: project.title.clone(),
        })?;
    let patch = ProjectNotePatch {
        title: command.edits.title().clone(),
        content: command.edits.content().clone(),
        domain: command.edits.domain().clone(),
        tags: command.edits.tags().clone(),
        sources: command.edits.sources().clone(),
        verified: command.edits.verified().clone(),
    };
    let mut title = existing.title;
    patch.title.clone().apply(&mut title);
    store
        .update_note(&project, &id, patch)
        .map_err(|error| EditNoteError::Store(anyhow::Error::new(error)))?;
    Ok(MutatedNote {
        id,
        project: project.title,
        title,
    })
}
