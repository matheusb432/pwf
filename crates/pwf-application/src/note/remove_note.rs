//! Removes one note from a managed project.

use pwf_models::{
    note::NoteSelector,
    project::{ProjectId, ProjectName},
};
use pwf_wire::{
    confirmation::RemoveNoteConfirmation,
    note::{MutatedNote, RemoveNote, RemovedNoteOutcome},
};

use crate::{
    ports::{
        confirmation::{ConfirmationClient, ConfirmationClientError},
        project_note::ProjectNotes,
    },
    project::{get_active_project, get_project::GetProjectError},
};

#[derive(Debug, thiserror::Error)]
pub enum RemoveNoteError {
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
    #[error(transparent)]
    Confirmation(#[from] ConfirmationClientError),
}

/// Resolves and deletes one note after confirming its representation exists.
#[cqrsy::command]
pub async fn execute(
    command: RemoveNote,
    store: &impl ProjectNotes,
    pool: &sqlx::SqlitePool,
    confirmation_client: &mut dyn ConfirmationClient<Confirmation = RemoveNoteConfirmation>,
) -> Result<RemovedNoteOutcome, RemoveNoteError> {
    let project = get_active_project::execute(command.project_id, pool).await?;
    let id =
        command
            .selector
            .resolve(&project.id)
            .ok_or_else(|| RemoveNoteError::ProjectMismatch {
                selector: command.selector,
                project_id: project.id.clone(),
            })?;
    let note = store
        .get_note(&project, &id)
        .map_err(|error| RemoveNoteError::Store(anyhow::Error::new(error)))?;
    let Some(note) = note else {
        return Err(RemoveNoteError::NoSuchNote {
            id,
            project: project.title,
        });
    };
    let confirmation = RemoveNoteConfirmation {
        note_identifier: id.clone(),
        project: project.title.clone(),
        title: note.title.clone(),
    };
    if !confirmation_client.confirm(&confirmation).await? {
        return Ok(RemovedNoteOutcome::Aborted { note_id: id });
    }
    store
        .delete_note(&project, &id)
        .map_err(|error| RemoveNoteError::Store(anyhow::Error::new(error)))?;
    Ok(RemovedNoteOutcome::Removed(MutatedNote {
        id,
        project: project.title,
        title: note.title,
    }))
}
