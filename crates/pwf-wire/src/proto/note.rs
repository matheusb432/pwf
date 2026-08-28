//! Explicit protobuf mappings for note operations.

use std::{fmt::Display, num::NonZeroUsize, str::FromStr};

use pwf_models::{
    AppDate,
    note::{NoteDomain, NoteId, NoteSelector, NoteTitle, NoteVerification, NoteWhy},
};
use tonic::Status;

use super::{invalid, parse};
use crate::{note, v1};

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

pub fn remove_note_request(request: v1::RemoveNoteRequest) -> Result<note::RemoveNote, Status> {
    let v1::RemoveNoteRequest {
        project_selector,
        selector,
    } = request;
    Ok(note::RemoveNote {
        project_selector: parse("project_selector", &project_selector)?,
        selector: parse::<NoteSelector>("selector", &selector)?,
    })
}

pub fn update_note_request(request: v1::UpdateNoteRequest) -> Result<note::UpdateNote, Status> {
    let v1::UpdateNoteRequest {
        project_selector,
        selector,
        title,
    } = request;
    Ok(note::UpdateNote {
        project_selector: parse("project_selector", &project_selector)?,
        selector: parse::<NoteSelector>("selector", &selector)?,
        title: parse::<NoteTitle>("title", &title)?,
    })
}

#[must_use]
pub fn add_note_response(note: note::NoteSummary) -> v1::AddNoteResponse {
    let note::NoteSummary { id, title } = note;
    v1::AddNoteResponse {
        id: id.to_string(),
        title: title.to_string(),
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
pub fn remove_note_response(id: &NoteId) -> v1::RemoveNoteResponse {
    v1::RemoveNoteResponse { id: id.to_string() }
}

#[must_use]
pub fn update_note_response(note: note::NoteSummary) -> v1::UpdateNoteResponse {
    let note::NoteSummary { id, title } = note;
    v1::UpdateNoteResponse {
        id: id.to_string(),
        title: title.to_string(),
    }
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
