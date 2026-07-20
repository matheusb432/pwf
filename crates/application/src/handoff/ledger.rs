use std::error::Error;

use pwf_domain::handoff::HandoffStatus;

use crate::{
    AppDbStore, HandoffDocumentStore, HandoffLedger, HandoffLedgerRow, HandoffLedgerWrite,
    HandoffLocation, HandoffScope,
};

type StoreError = Box<dyn Error + Send + Sync>;

#[derive(Debug, thiserror::Error)]
pub(crate) enum RebuildLedgerError {
    #[error("{0}")]
    ReadDocuments(#[source] StoreError),
    #[error("{0}")]
    WriteLedger(#[source] StoreError),
}

pub(crate) fn rebuild<S>(
    scope: &HandoffScope,
    store: &S,
) -> Result<HandoffLedger, RebuildLedgerError>
where
    S: HandoffDocumentStore + AppDbStore<HandoffLedger>,
{
    let documents = store
        .list_location(scope, HandoffLocation::Active)
        .map_err(|error| RebuildLedgerError::ReadDocuments(Box::new(error)))?;
    let mut rows = documents
        .into_iter()
        .filter(|document| {
            document.location == HandoffLocation::Active
                && document.status == Some(HandoffStatus::Active)
        })
        .map(|document| HandoffLedgerRow {
            pending_work_identifier: document
                .pending_work_identifier_raw
                .unwrap_or_else(|| file_stem(&document.identifier.file_name)),
            title: document.title,
            file_name: document.identifier.file_name,
            goals_completed: document.goals_completed,
            goals_total: document.goals_total,
            created: document
                .created
                .map(|created| created.as_str().to_string())
                .unwrap_or_default(),
        })
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        right
            .created
            .cmp(&left.created)
            .then_with(|| right.file_name.cmp(&left.file_name))
    });
    <S as AppDbStore<HandoffLedger>>::insert(store, scope, HandoffLedgerWrite { rows })
        .map_err(|error| RebuildLedgerError::WriteLedger(Box::new(error)))
}

fn file_stem(file_name: &str) -> String {
    file_name
        .strip_suffix(".md")
        .unwrap_or(file_name)
        .to_string()
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, time::SystemTime};

    use pwf_domain::{
        handoff::HandoffStatus,
        pending_work::{ProjectName, Timestamp},
    };

    use super::rebuild;
    use crate::{
        HandoffDocument, HandoffDocumentIdentifier, HandoffLocation, HandoffScope,
        testing::InMemoryStore,
    };

    fn document(
        file_name: &str,
        created: &str,
        status: HandoffStatus,
        pending_work_identifier_raw: Option<&str>,
    ) -> HandoffDocument {
        HandoffDocument {
            identifier: HandoffDocumentIdentifier {
                file_name: file_name.to_string(),
                location: HandoffLocation::Active,
            },
            location: HandoffLocation::Active,
            project: Some(ProjectName::try_new("test-project").unwrap()),
            title: file_name
                .trim_end_matches(".md")
                .trim_start_matches(created)
                .trim_start_matches('-')
                .replace('-', " "),
            status: Some(status),
            created: Some(Timestamp::new(created)),
            completed: None,
            pending_work_identifier_raw: pending_work_identifier_raw.map(str::to_string),
            goals_completed: 1,
            goals_total: 2,
            body: String::new(),
            source: String::new(),
            locator: PathBuf::from("/unused").join(file_name),
            modified_timestamp: SystemTime::UNIX_EPOCH,
        }
    }

    #[test]
    fn rebuild_filters_active_documents_and_sorts_newest_first() {
        let scope = HandoffScope {
            repository_root: PathBuf::from("/repo/test-project"),
        };
        let store = InMemoryStore::default().with_handoff_documents(
            scope.clone(),
            vec![
                document(
                    "2026-01-01-alpha.md",
                    "2026-01-01",
                    HandoffStatus::Active,
                    Some("TST-0001"),
                ),
                document(
                    "2026-01-02-beta.md",
                    "2026-01-02",
                    HandoffStatus::Done,
                    Some("TST-0002"),
                ),
                document(
                    "2026-01-03-zeta.md",
                    "2026-01-03",
                    HandoffStatus::Active,
                    None,
                ),
                document(
                    "2026-01-03-gamma.md",
                    "2026-01-03",
                    HandoffStatus::Active,
                    Some("TST-0003"),
                ),
            ],
        );

        let ledger = rebuild(&scope, &store).unwrap();

        let gamma = ledger.source.find("TST-0003").unwrap();
        let zeta = ledger.source.find("2026-01-03-zeta").unwrap();
        let alpha = ledger.source.find("TST-0001").unwrap();
        assert!(zeta < gamma && gamma < alpha, "{}", ledger.source);
        assert!(!ledger.source.contains("TST-0002"));
    }
}
