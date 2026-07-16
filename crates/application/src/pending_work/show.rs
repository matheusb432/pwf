use pwf_domain::pending_work::ProjectRegistry;

use crate::{
    AppDbStore, Materialization, PendingWorkItem,
    pending_work::resolve::{ResolvePendingWorkError, ResolvedItem, resolve_record},
};

pub type ShowPendingWorkError = ResolvePendingWorkError;

#[derive(Debug, Clone)]
pub struct ShowPendingWorkItem {
    pub id: String,
}

/// `pwf show <id>` streams the note's full markdown source verbatim — the same
/// lookup as `resolve`, emitting [`ResolvedItem::markdown`]. A missing-note
/// wikilink has no source to stream, so it is rejected with
/// [`ResolvePendingWorkError::NoteFileMissing`] instead of an empty success
/// (the legacy read errored here too).
#[cqrsy::handler(query)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "the cqrsy show operation owns its request by contract"
)]
pub fn execute(
    query: ShowPendingWorkItem,
    store: &impl AppDbStore<PendingWorkItem>,
    projects: &ProjectRegistry,
) -> Result<ResolvedItem, ShowPendingWorkError> {
    let record = resolve_record(store, projects, &query.id)?;
    if matches!(record.materialization, Materialization::MissingNote { .. }) {
        return Err(ShowPendingWorkError::NoteFileMissing {
            path: record.locator,
        });
    }
    Ok(ResolvedItem::from(record))
}

#[cfg(test)]
mod tests {
    use super::{ShowPendingWorkError, ShowPendingWorkItem, execute};
    use crate::pending_work::resolve::testing::{PWF_0001_SOURCE, staged, staged_ghost};

    #[test]
    fn show_streams_source_verbatim() {
        let (store, registry) = staged();

        let shown = execute(
            ShowPendingWorkItem {
                id: "PWF-0001".to_string(),
            },
            &store,
            &registry,
        )
        .unwrap();

        assert_eq!(shown.markdown, PWF_0001_SOURCE);
    }

    #[test]
    fn show_rejects_missing_note_wikilink_instead_of_empty_success() {
        let (store, registry) = staged_ghost();

        let error = execute(
            ShowPendingWorkItem {
                id: "PWF-0002".to_string(),
            },
            &store,
            &registry,
        )
        .unwrap_err();

        assert!(matches!(
            error,
            ShowPendingWorkError::NoteFileMissing { ref path }
                if path == "/notes/pwf/PWF-0002.md"
        ));
    }
}
