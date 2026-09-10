//! Lists one managed project's notes.

use pwf_wire::note::{ListNotes, ListedNotes, NoteListLimit};

use crate::{
    ports::project_note::ProjectNotes,
    project::{get_active_project, get_project::GetProjectError},
};

const DEFAULT_NOTE_COUNT: usize = 10;

#[derive(Debug, thiserror::Error)]
pub enum ListNotesError {
    #[error(transparent)]
    GetProject(#[from] GetProjectError),
    #[error(transparent)]
    Store(anyhow::Error),
}

/// Reads, orders, and caps one project's notes.
#[cqrsy::query]
pub async fn execute(
    query: ListNotes,
    store: &impl ProjectNotes,
    pool: &sqlx::SqlitePool,
) -> Result<ListedNotes, ListNotesError> {
    let project = get_active_project::execute(query.project_id, pool).await?;
    let mut notes = store
        .list_notes(&project)
        .map_err(|error| ListNotesError::Store(anyhow::Error::new(error)))?;
    notes.sort_by_key(|note| std::cmp::Reverse(note.id.number()));
    let shown = shown_count(query.limit, notes.len());
    let hidden = notes.len() - shown;
    let notes = notes.into_iter().take(shown).collect();
    Ok(ListedNotes {
        project: project.title.clone(),
        notes,
        hidden,
    })
}

fn shown_count(limit: NoteListLimit, available: usize) -> usize {
    match limit {
        NoteListLimit::Default => DEFAULT_NOTE_COUNT.min(available),
        NoteListLimit::Unlimited => available,
        NoteListLimit::AtMost(limit) => limit.get().min(available),
    }
}
