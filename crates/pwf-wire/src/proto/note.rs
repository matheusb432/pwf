//! Explicit protobuf mappings for note operations.

use std::{fmt::Display, num::NonZeroUsize, str::FromStr};

use pwf_models::{
    AppDate,
    note::{NoteDomain, NoteSelector, NoteSource, NoteTag, NoteTitle, NoteVerification},
};
use tonic::Status;

use super::{collection_edit, invalid, parse, required};
use crate::{confirmation, note, patch_field::PatchField, pb};

pub fn add_note_request(request: pb::AddNoteRequest) -> Result<note::AddNote, Status> {
    Ok(note::AddNote {
        project_id: parse("project_id", &request.project_id)?,
        title: parse("title", &request.title)?,
        content: parse("content", &request.content)?,
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

pub fn list_notes_request(request: pb::ListNotesRequest) -> Result<note::ListNotes, Status> {
    let pb::ListNotesRequest {
        project_id,
        limit_kind,
        limit,
    } = request;
    let limit = match pb::NoteListLimitKind::try_from(limit_kind).ok() {
        Some(pb::NoteListLimitKind::Default) => note::NoteListLimit::Default,
        Some(pb::NoteListLimitKind::Unlimited) => note::NoteListLimit::Unlimited,
        Some(pb::NoteListLimitKind::AtMost) => note::NoteListLimit::AtMost(
            usize::try_from(limit)
                .ok()
                .and_then(NonZeroUsize::new)
                .ok_or_else(|| invalid("limit", "must be a positive platform-sized integer"))?,
        ),
        Some(pb::NoteListLimitKind::Unspecified) | None => {
            return Err(invalid("limit_kind", "must be specified"));
        }
    };
    Ok(note::ListNotes {
        project_id: parse("project_id", &project_id)?,
        limit,
    })
}

pub fn update_note_request(request: pb::UpdateNoteRequest) -> Result<note::EditNote, Status> {
    let pb::UpdateNoteRequest {
        project_id,
        selector,
        title,
        content,
        domain,
        tags,
        sources,
        verified,
    } = request;
    let edits = note::NoteEdits::try_new(
        title
            .as_deref()
            .map(|value| parse::<NoteTitle>("title", value))
            .transpose()?
            .into(),
        content
            .as_deref()
            .map(|value| parse("content", value))
            .transpose()?
            .into(),
        note_patch_field("domain", domain)?,
        collection_edit(tags, note_tag_values)?,
        collection_edit(sources, note_source_values)?,
        note_patch_field("verified", verified)?,
    )
    .map_err(|error| invalid("edits", error))?;
    Ok(note::EditNote {
        project_id: parse("project_id", &project_id)?,
        selector: parse::<NoteSelector>("selector", &selector)?,
        edits,
    })
}

pub fn delete_note_start(start: &pb::DeleteNoteStart) -> Result<note::RemoveNote, Status> {
    Ok(note::RemoveNote {
        project_id: parse("project_id", &start.project_id)?,
        selector: parse("selector", &start.selector)?,
    })
}

#[must_use]
pub fn add_note_response(note: &note::MutatedNote) -> pb::AddNoteResponse {
    pb::AddNoteResponse {
        id: note.id.to_string(),
        title: note.title.to_string(),
        project: note.project.to_string(),
    }
}

#[must_use]
pub fn list_notes_response(notes: note::ListedNotes) -> pb::ListNotesResponse {
    pb::ListNotesResponse {
        project: notes.project.to_string(),
        notes: notes
            .notes
            .into_iter()
            .map(|note| pb::ListedNote {
                is_verified: note.is_verified(),
                id: note.id.to_string(),
                title: note.title.to_string(),
            })
            .collect(),
        hidden: notes.hidden as u64,
    }
}

#[must_use]
pub fn update_note_response(note: &note::MutatedNote) -> pb::UpdateNoteResponse {
    pb::UpdateNoteResponse {
        id: note.id.to_string(),
        title: note.title.to_string(),
        project: note.project.to_string(),
    }
}

#[must_use]
pub fn delete_note_confirmation(
    confirmation: &confirmation::RemoveNoteConfirmation,
) -> pb::DeleteNoteConfirmation {
    pb::DeleteNoteConfirmation {
        note_id: confirmation.note_identifier.to_string(),
        project: confirmation.project.to_string(),
        title: confirmation.title.to_string(),
    }
}

#[must_use]
pub fn delete_note_result(outcome: note::RemovedNoteOutcome) -> pb::DeleteNoteResult {
    let outcome = match outcome {
        note::RemovedNoteOutcome::Removed(note) => {
            pb::delete_note_result::Outcome::Deleted(pb::DeletedNote {
                id: note.id.to_string(),
                project: note.project.to_string(),
                title: note.title.to_string(),
            })
        }
        note::RemovedNoteOutcome::Aborted { note_id } => {
            pb::delete_note_result::Outcome::Aborted(pb::AbortedNoteOperation {
                id: note_id.to_string(),
            })
        }
    };
    pb::DeleteNoteResult {
        outcome: Some(outcome),
    }
}

fn note_patch_field<T>(
    field: &str,
    update: Option<pb::StringPatchField>,
) -> Result<PatchField<T>, Status>
where
    T: FromStr,
    T::Err: Display,
{
    let Some(update) = update else {
        return Ok(PatchField::NoAction);
    };
    match required(field, update.operation)? {
        pb::string_patch_field::Operation::Set(value) => parse(field, &value).map(PatchField::Set),
        pb::string_patch_field::Operation::Clear(_) => Ok(PatchField::Clear),
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
        patch_field::PatchField,
        pb::{
            ClearField, StringCollectionEdit, StringPatchField, StringValues, UpdateNoteRequest,
            string_collection_edit, string_patch_field,
        },
        set_field::SetField,
    };

    fn request() -> UpdateNoteRequest {
        UpdateNoteRequest {
            project_id: "foo".to_string(),
            selector: "1".to_string(),
            title: None,
            content: None,
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
            tags: Some(StringCollectionEdit {
                operation: Some(string_collection_edit::Operation::Append(StringValues {
                    values: vec!["rust".to_string()],
                })),
            }),
            sources: Some(StringCollectionEdit {
                operation: Some(string_collection_edit::Operation::Clear(ClearField {})),
            }),
            verified: Some(StringPatchField {
                operation: Some(string_patch_field::Operation::Set("2026-08-30".to_string())),
            }),
            ..request()
        })
        .unwrap();

        assert!(matches!(command.edits.title(), SetField::NoAction));
        assert!(matches!(
            command.edits.content(),
            SetField::Set(value) if value.as_ref() == "New content"
        ));
        assert!(matches!(command.edits.domain(), PatchField::NoAction));
        assert!(matches!(
            command.edits.tags(),
            CollectionEdit::Append(values) if values[0].as_ref() == "rust"
        ));
        assert!(matches!(command.edits.sources(), CollectionEdit::Clear));
        assert!(matches!(
            command.edits.verified(),
            PatchField::Set(value) if value.as_ref() == "2026-08-30"
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
