//! Lists note summaries under one cap across the selected active projects.

use std::collections::HashMap;

use pwf_client::{PwfClient, pb};

use crate::OperationError;

pub(crate) struct NoteListing {
    pub(crate) entries: HashMap<String, String>,
    pub(crate) hidden: u64,
}

pub(crate) async fn execute(
    client: &PwfClient,
    projects: &[pb::Project],
    limit: usize,
) -> Result<NoteListing, OperationError> {
    let mut listing = NoteListing {
        entries: HashMap::new(),
        hidden: 0,
    };
    for project in projects {
        let remaining = limit - listing.entries.len();
        // Even after the cap, one summary supplies this project's exact hidden count.
        let response = client
            .note()
            .list_notes(pb::ListNotesRequest {
                project_id: project.id.clone(),
                limit_kind: pb::NoteListLimitKind::AtMost as i32,
                limit: remaining.max(1) as u64,
            })
            .await?;
        let shown = remaining.min(response.notes.len());
        listing.hidden += response.hidden + (response.notes.len() - shown) as u64;
        for note in response.notes.into_iter().take(shown) {
            let verified = if note.is_verified { " [verified]" } else { "" };
            listing.entries.insert(
                note.id.clone(),
                format!("{} {}{verified}", note.id, note.title),
            );
        }
    }
    Ok(listing)
}
