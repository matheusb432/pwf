//! Explicit protobuf mappings for note operations.

use std::{fmt::Display, num::NonZeroUsize, str::FromStr};

use pwf_models::{
    AppDate,
    note::{NoteDomain, NoteSelector, NoteSource, NoteTag, NoteTitle, NoteVerification, NoteWhy},
};
use tonic::Status;

use super::{collection_edit, invalid, parse, required};
use crate::{confirmation, field_update::FieldUpdate, note, v1};

pub fn add_note_request(request: v1::AddNoteRequest) -> Result<note::AddNote, Status> {
    Ok(note::AddNote {
        project_selector: parse("project_selector", &request.project_selector)?,
        title: parse("title", &request.title)?,
        content: parse("content", &request.content)?,
        why: request
            .why
            .as_deref()
            .map(|value| parse::<NoteWhy>("why", value))
            .transpose()?,
        domain: request
            .domain
            .as_deref()
            .map(|value| parse::<NoteDomain>("domain", value))
            .transpose()?,
        tags: parse_repeated("tags", request.tags)?,
        sources: parse_repeated("sources", request.sources)?,
        verified: request
            .verified
            .as_deref()
            .map(|value| parse::<NoteVerification>("verified", value))
            .transpose()?,
        date: request
            .date
            .as_deref()
            .map(|value| parse::<AppDate>("date", value))
            .transpose()?,
    })
}

pub fn list_notes_request(request: v1::ListNotesRequest) -> Result<note::ListNotes, Status> {
    let v1::ListNotesRequest {
        project_selector,
        limit_kind,
        limit,
    } = request;
    let limit = match v1::NoteListLimitKind::try_from(limit_kind).ok() {
        Some(v1::NoteListLimitKind::Default) => note::NoteListLimit::Default,
        Some(v1::NoteListLimitKind::Unlimited) => note::NoteListLimit::Unlimited,
        Some(v1::NoteListLimitKind::AtMost) => note::NoteListLimit::AtMost(
            usize::try_from(limit)
                .ok()
                .and_then(NonZeroUsize::new)
                .ok_or_else(|| invalid("limit", "must be a positive platform-sized integer"))?,
        ),
        Some(v1::NoteListLimitKind::Unspecified) | None => {
            return Err(invalid("limit_kind", "must be specified"));
        }
    };
    Ok(note::ListNotes {
        project_selector: parse("project_selector", &project_selector)?,
        limit,
    })
}

pub fn update_note_request(request: v1::UpdateNoteRequest) -> Result<note::EditNote, Status> {
    let v1::UpdateNoteRequest {
        project_selector,
        selector,
        title,
        content,
        why,
        domain,
        tags,
        sources,
        verified,
    } = request;
    let edits = note::NoteEdits::try_new(
        title
            .as_deref()
            .map(|value| parse::<NoteTitle>("title", value))
            .transpose()?,
        content
            .as_deref()
            .map(|value| parse("content", value))
            .transpose()?,
        note_field_update("why", why)?,
        note_field_update("domain", domain)?,
        collection_edit(tags, note_tag_values)?,
        collection_edit(sources, note_source_values)?,
        note_field_update("verified", verified)?,
    )
    .map_err(|error| invalid("edits", error))?;
    Ok(note::EditNote {
        project_selector: parse("project_selector", &project_selector)?,
        selector: parse::<NoteSelector>("selector", &selector)?,
        edits,
    })
}

pub fn delete_note_start(start: &v1::DeleteNoteStart) -> Result<note::RemoveNote, Status> {
    Ok(note::RemoveNote {
        project_selector: parse("project_selector", &start.project_selector)?,
        selector: parse("selector", &start.selector)?,
    })
}

#[must_use]
pub fn add_note_response(note: &note::MutatedNote) -> v1::AddNoteResponse {
    v1::AddNoteResponse {
        id: note.id.to_string(),
        title: note.title.to_string(),
        project: note.project.to_string(),
    }
}

#[must_use]
pub fn list_notes_response(notes: note::ListedNotes) -> v1::ListNotesResponse {
    v1::ListNotesResponse {
        project: notes.project.to_string(),
        notes: notes
            .notes
            .into_iter()
            .map(|note| v1::ListedNote {
                id: note.id.to_string(),
                title: note.title.to_string(),
            })
            .collect(),
        hidden: notes.hidden as u64,
    }
}

#[must_use]
pub fn update_note_response(note: &note::MutatedNote) -> v1::UpdateNoteResponse {
    v1::UpdateNoteResponse {
        id: note.id.to_string(),
        title: note.title.to_string(),
        project: note.project.to_string(),
    }
}

#[must_use]
pub fn delete_note_confirmation(
    confirmation: &confirmation::RemoveNoteConfirmation,
) -> v1::DeleteNoteConfirmation {
    v1::DeleteNoteConfirmation {
        note_id: confirmation.note_identifier.to_string(),
        project: confirmation.project.to_string(),
        title: confirmation.title.to_string(),
    }
}

#[must_use]
pub fn delete_note_result(outcome: note::RemovedNoteOutcome) -> v1::DeleteNoteResult {
    let outcome = match outcome {
        note::RemovedNoteOutcome::Removed(note) => {
            v1::delete_note_result::Outcome::Deleted(v1::DeletedNote {
                id: note.id.to_string(),
                project: note.project.to_string(),
                title: note.title.to_string(),
            })
        }
        note::RemovedNoteOutcome::Aborted { note_id } => {
            v1::delete_note_result::Outcome::Aborted(v1::AbortedNoteOperation {
                id: note_id.to_string(),
            })
        }
    };
    v1::DeleteNoteResult {
        outcome: Some(outcome),
    }
}

fn note_field_update<T>(
    field: &str,
    update: Option<v1::StringFieldUpdate>,
) -> Result<FieldUpdate<T>, Status>
where
    T: FromStr,
    T::Err: Display,
{
    let Some(update) = update else {
        return Ok(FieldUpdate::Unchanged);
    };
    match required(field, update.operation)? {
        v1::string_field_update::Operation::Update(value) => {
            parse(field, &value).map(FieldUpdate::Update)
        }
        v1::string_field_update::Operation::Clear(_) => Ok(FieldUpdate::Clear),
    }
}

fn note_tag_values(values: Vec<String>) -> Result<Option<Vec<NoteTag>>, Status> {
    nonempty_repeated("tags", values)
}

fn note_source_values(values: Vec<String>) -> Result<Option<Vec<NoteSource>>, Status> {
    nonempty_repeated("sources", values)
}

fn nonempty_repeated<T>(field: &str, values: Vec<String>) -> Result<Option<Vec<T>>, Status>
where
    T: FromStr,
    T::Err: Display,
{
    if values.is_empty() {
        return Ok(None);
    }
    parse_repeated(field, values).map(Some)
}

fn parse_repeated<T>(field: &str, values: Vec<String>) -> Result<Vec<T>, Status>
where
    T: FromStr,
    T::Err: Display,
{
    values
        .into_iter()
        .enumerate()
        .map(|(index, value)| parse(&format!("{field}[{index}]"), &value))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::update_note_request;
    use crate::{
        collection_edit::CollectionEdit,
        field_update::FieldUpdate,
        v1::{
            ClearField, StringCollectionEdit, StringFieldUpdate, StringValues, UpdateNoteRequest,
            string_collection_edit, string_field_update,
        },
    };

    fn request() -> UpdateNoteRequest {
        UpdateNoteRequest {
            project_selector: "foo".to_string(),
            selector: "1".to_string(),
            title: None,
            content: None,
            why: None,
            domain: None,
            tags: None,
            sources: None,
            verified: None,
        }
    }

    #[test]
    fn partial_edits_decode_explicit_presence_and_collection_operations() {
        let command = update_note_request(UpdateNoteRequest {
            content: Some("New content".to_string()),
            why: Some(StringFieldUpdate {
                operation: Some(string_field_update::Operation::Clear(ClearField {})),
            }),
            tags: Some(StringCollectionEdit {
                operation: Some(string_collection_edit::Operation::Append(StringValues {
                    values: vec!["rust".to_string()],
                })),
            }),
            sources: Some(StringCollectionEdit {
                operation: Some(string_collection_edit::Operation::Clear(ClearField {})),
            }),
            verified: Some(StringFieldUpdate {
                operation: Some(string_field_update::Operation::Update(
                    "2026-08-30".to_string(),
                )),
            }),
            ..request()
        })
        .unwrap();

        assert!(command.edits.title().is_none());
        assert_eq!(command.edits.content().unwrap().as_ref(), "New content");
        assert!(matches!(command.edits.why(), FieldUpdate::Clear));
        assert!(matches!(command.edits.domain(), FieldUpdate::Unchanged));
        assert!(matches!(
            command.edits.tags(),
            CollectionEdit::Append(values) if values[0].as_ref() == "rust"
        ));
        assert!(matches!(command.edits.sources(), CollectionEdit::Clear));
        assert!(matches!(
            command.edits.verified(),
            FieldUpdate::Update(value) if value.as_ref() == "2026-08-30"
        ));
    }

    #[test]
    fn empty_edit_is_rejected() {
        let error = update_note_request(request()).unwrap_err();

        assert_eq!(error.code(), tonic::Code::InvalidArgument);
        assert!(error.message().contains("nothing to edit"), "{error}");
    }

    #[test]
    fn append_requires_at_least_one_collection_value() {
        let error = update_note_request(UpdateNoteRequest {
            tags: Some(StringCollectionEdit {
                operation: Some(string_collection_edit::Operation::Append(StringValues {
                    values: Vec::new(),
                })),
            }),
            ..request()
        })
        .unwrap_err();

        assert_eq!(error.code(), tonic::Code::InvalidArgument);
        assert!(error.message().contains("cannot be empty"), "{error}");
    }
}
