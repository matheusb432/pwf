//! Adds one note to a managed project.

use pwf_models::{note::NoteId, project::ProjectName, task::TaskTimestampError};
use pwf_wire::note::{AddNote, MutatedNote};

use crate::{
    ports::{
        clock::Clock,
        project_note::{NewProjectNote, ProjectNotes},
    },
    project::{get_active_project, get_project::GetProjectError},
};

#[derive(Debug, thiserror::Error)]
pub enum AddNoteError {
    #[error(transparent)]
    GetProject(#[from] GetProjectError),
    #[error("Project '{project}' has no available four-digit note identifiers.")]
    IdentifierExhausted { project: ProjectName },
    #[error(transparent)]
    Store(anyhow::Error),
    #[error("cannot read the current date: {0}")]
    Clock(#[from] TaskTimestampError),
}

/// Validates, allocates, and persists one project note.
#[cqrsy::command]
pub async fn execute(
    command: AddNote,
    store: &impl ProjectNotes,
    pool: &sqlx::SqlitePool,
    clock: &impl Clock,
) -> Result<MutatedNote, AddNoteError> {
    let project = get_active_project::execute(command.project_id, pool).await?;
    let notes = store
        .list_notes(&project)
        .map_err(|error| AddNoteError::Store(anyhow::Error::new(error)))?;
    let next_number = notes
        .iter()
        .map(|note| note.id.number())
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .filter(|number| *number <= 9_999)
        .ok_or_else(|| AddNoteError::IdentifierExhausted {
            project: project.title.clone(),
        })?;
    let id = NoteId::try_new(format!("{}-NOTE-{next_number:04}", project.id)).map_err(|_| {
        AddNoteError::IdentifierExhausted {
            project: project.title.clone(),
        }
    })?;
    let created = match command.date {
        Some(date) => date,
        None => clock.today()?,
    };
    let created = store
        .insert_note(
            &project,
            NewProjectNote {
                id,
                title: command.title,
                content: command.content,
                domain: command.domain,
                tags: command.tags,
                sources: command.sources,
                verified: command.verified,
                created,
            },
        )
        .map_err(|error| AddNoteError::Store(anyhow::Error::new(error)))?;
    Ok(MutatedNote {
        id: created.id,
        project: project.title,
        title: created.title,
    })
}
