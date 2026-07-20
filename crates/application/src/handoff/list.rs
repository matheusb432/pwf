use std::error::Error;

use crate::{AppDbStore, HandoffLedger, HandoffLedgerIdentifier, HandoffScope};

type StoreError = Box<dyn Error + Send + Sync>;

/// Requests the persisted ledger for one repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListHandoffs {
    /// Repository whose ledger should be read.
    pub scope: HandoffScope,
}

/// Distinguishes a missing ledger from present Markdown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListedHandoffs {
    /// Verbatim persisted ledger Markdown.
    Ledger {
        /// Complete ledger source.
        markdown: String,
    },
    /// The ledger path does not exist.
    LedgerMissing,
}

/// Reports a ledger read failure without conflating it with absence.
#[derive(Debug, thiserror::Error)]
pub enum ListHandoffsError {
    /// The adapter could not read an existing ledger.
    #[error("{source}")]
    ReadLedger {
        /// Concrete adapter failure.
        #[source]
        source: StoreError,
    },
}

/// Reads one handoff ledger through its singleton record port.
///
/// # Errors
///
/// Returns [`ListHandoffsError`] when the storage adapter cannot read the ledger.
#[cqrsy::query]
pub fn execute<S>(command: ListHandoffs, store: &S) -> Result<ListedHandoffs, ListHandoffsError>
where
    S: AppDbStore<HandoffLedger>,
{
    let ListHandoffs { scope } = command;
    let ledger = <S as AppDbStore<HandoffLedger>>::get(store, &scope, &HandoffLedgerIdentifier)
        .map_err(|error| ListHandoffsError::ReadLedger {
            source: Box::new(error),
        })?;
    Ok(match ledger {
        Some(ledger) => ListedHandoffs::Ledger {
            markdown: ledger.source,
        },
        None => ListedHandoffs::LedgerMissing,
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{ListHandoffs, ListedHandoffs, execute};
    use crate::{
        AppDbStore, HandoffLedger, HandoffLedgerIdentifier, HandoffLedgerRow, HandoffLedgerWrite,
        HandoffScope, testing::InMemoryStore,
    };

    fn scope() -> HandoffScope {
        HandoffScope {
            repository_root: PathBuf::from("/repo/test-project"),
        }
    }

    #[test]
    fn list_distinguishes_a_missing_ledger_from_present_markdown() {
        let store = InMemoryStore::default();

        assert_eq!(
            execute(ListHandoffs { scope: scope() }, &store,).unwrap(),
            ListedHandoffs::LedgerMissing
        );

        <InMemoryStore as AppDbStore<HandoffLedger>>::insert(
            &store,
            &scope(),
            HandoffLedgerWrite {
                rows: vec![HandoffLedgerRow {
                    pending_work_identifier: "TST-0001".to_string(),
                    title: "Managed Flow".to_string(),
                    file_name: "2026-01-01-managed-flow.md".to_string(),
                    goals_completed: 0,
                    goals_total: 1,
                    created: "2026-01-01".to_string(),
                }],
            },
        )
        .unwrap();

        let listed = execute(ListHandoffs { scope: scope() }, &store).unwrap();
        let ListedHandoffs::Ledger { markdown } = listed else {
            panic!("expected the persisted ledger")
        };
        assert!(markdown.contains("| TST-0001 | [Managed Flow]"));
    }

    #[test]
    fn ledger_identifier_is_a_singleton_key() {
        let store = InMemoryStore::default();

        assert!(
            <InMemoryStore as AppDbStore<HandoffLedger>>::get(
                &store,
                &scope(),
                &HandoffLedgerIdentifier,
            )
            .unwrap()
            .is_none()
        );
    }
}
