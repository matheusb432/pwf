use std::{fmt::Write as _, path::PathBuf};

use pwf_application::{
    AppRecordStore, HandoffLedger, HandoffLedgerIdentifier, HandoffLedgerWrite, HandoffScope,
};

use super::{ObsidianStore, ObsidianStoreError};

impl AppRecordStore<HandoffLedger> for ObsidianStore {
    type Error = ObsidianStoreError;

    fn get(
        &self,
        scope: &HandoffScope,
        _identifier: &HandoffLedgerIdentifier,
    ) -> Result<Option<HandoffLedger>, Self::Error> {
        let path = ledger_path(scope);
        match std::fs::read_to_string(&path) {
            Ok(source) => Ok(Some(HandoffLedger {
                source,
                locator: path,
            })),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(ObsidianStoreError::ReadHandoffLedger { path, source }),
        }
    }

    fn list(&self, scope: &HandoffScope) -> Result<Vec<HandoffLedger>, Self::Error> {
        Ok(
            <Self as AppRecordStore<HandoffLedger>>::get(self, scope, &HandoffLedgerIdentifier)?
                .into_iter()
                .collect(),
        )
    }

    fn insert(
        &self,
        scope: &HandoffScope,
        new: HandoffLedgerWrite,
    ) -> Result<HandoffLedger, Self::Error> {
        let path = ledger_path(scope);
        let source = render_ledger(&new);
        pwf_core::fs_atomic::write_text_atomic(&path, &source).map_err(|source| {
            ObsidianStoreError::WriteHandoffLedger {
                path: path.clone(),
                source,
            }
        })?;
        Ok(HandoffLedger {
            source,
            locator: path,
        })
    }

    fn update(
        &self,
        scope: &HandoffScope,
        _identifier: &HandoffLedgerIdentifier,
        patch: HandoffLedgerWrite,
    ) -> Result<(), Self::Error> {
        let _ = <Self as AppRecordStore<HandoffLedger>>::insert(self, scope, patch)?;
        Ok(())
    }

    fn delete(
        &self,
        scope: &HandoffScope,
        _identifier: &HandoffLedgerIdentifier,
    ) -> Result<(), Self::Error> {
        let path = ledger_path(scope);
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(ObsidianStoreError::RemoveHandoffLedger { path, source }),
        }
    }
}

fn ledger_path(scope: &HandoffScope) -> PathBuf {
    scope
        .repository_root
        .join("docs")
        .join("handoffs")
        .join("LEDGER.md")
}

fn render_ledger(write: &HandoffLedgerWrite) -> String {
    let mut source = String::from(
        "# Handoff ledger — active only\n\nOnly handoffs with status: active are listed.\n\n| ID | Handoff | Goals | Created |\n| --- | --- | --- | --- |\n",
    );
    for row in &write.rows {
        let _ = writeln!(
            source,
            "| {} | [{}]({}) | {}/{} | {} |",
            row.pending_work_identifier,
            row.title,
            row.file_name,
            row.goals_completed,
            row.goals_total,
            row.created
        );
    }
    source
}

#[cfg(test)]
mod tests {
    use std::{assert_matches, path::PathBuf};

    use pwf_application::{
        AppRecordStore, HandoffLedger, HandoffLedgerIdentifier, HandoffLedgerRow,
        HandoffLedgerWrite, HandoffScope,
    };

    use super::super::{ObsidianStore, ObsidianStoreError};

    fn store() -> ObsidianStore {
        ObsidianStore::new([])
    }

    fn scope(repository_root: PathBuf) -> HandoffScope {
        HandoffScope { repository_root }
    }

    fn write() -> HandoffLedgerWrite {
        HandoffLedgerWrite {
            rows: vec![HandoffLedgerRow {
                pending_work_identifier: "TST-0001".to_string(),
                title: "Managed Flow".to_string(),
                file_name: "2026-01-01-managed-flow.md".to_string(),
                goals_completed: 1,
                goals_total: 2,
                created: "2026-01-01".to_string(),
            }],
        }
    }

    #[test]
    fn ledger_missing_read_write_and_verbatim_read_are_distinct() {
        let temporary_directory = tempfile::tempdir().unwrap();
        let scope = scope(temporary_directory.path().join("repo"));
        let store = store();
        assert!(
            <ObsidianStore as AppRecordStore<HandoffLedger>>::get(
                &store,
                &scope,
                &HandoffLedgerIdentifier,
            )
            .unwrap()
            .is_none()
        );

        let inserted =
            <ObsidianStore as AppRecordStore<HandoffLedger>>::insert(&store, &scope, write())
                .unwrap();
        assert_eq!(
            inserted.source,
            "# Handoff ledger — active only\n\nOnly handoffs with status: active are listed.\n\n| ID | Handoff | Goals | Created |\n| --- | --- | --- | --- |\n| TST-0001 | [Managed Flow](2026-01-01-managed-flow.md) | 1/2 | 2026-01-01 |\n"
        );

        std::fs::write(&inserted.locator, "verbatim\r\nledger\r\n").unwrap();
        let read = <ObsidianStore as AppRecordStore<HandoffLedger>>::get(
            &store,
            &scope,
            &HandoffLedgerIdentifier,
        )
        .unwrap()
        .unwrap();
        assert_eq!(read.source, "verbatim\r\nledger\r\n");

        <ObsidianStore as AppRecordStore<HandoffLedger>>::delete(
            &store,
            &scope,
            &HandoffLedgerIdentifier,
        )
        .unwrap();
        assert!(!inserted.locator.exists());
    }

    #[test]
    fn unreadable_ledger_is_an_error_instead_of_missing() {
        let temporary_directory = tempfile::tempdir().unwrap();
        let scope = scope(temporary_directory.path().join("repo"));
        let ledger = scope.repository_root.join("docs/handoffs/LEDGER.md");
        std::fs::create_dir_all(&ledger).unwrap();

        let error = <ObsidianStore as AppRecordStore<HandoffLedger>>::get(
            &store(),
            &scope,
            &HandoffLedgerIdentifier,
        )
        .unwrap_err();

        assert_matches!(error, ObsidianStoreError::ReadHandoffLedger { .. });
    }

    #[test]
    fn ledger_write_error_displays_raw_io_and_retains_its_path() {
        let temporary_directory = tempfile::tempdir().unwrap();
        let scope = scope(temporary_directory.path().join("repo"));
        let ledger_path = scope.repository_root.join("docs/handoffs/LEDGER.md");
        std::fs::create_dir_all(&ledger_path).unwrap();

        let error =
            <ObsidianStore as AppRecordStore<HandoffLedger>>::insert(&store(), &scope, write())
                .unwrap_err();

        let display = error.to_string();
        let ObsidianStoreError::WriteHandoffLedger { path, source } = error else {
            panic!("expected a typed ledger write failure");
        };
        assert_eq!(path, ledger_path);
        assert_eq!(display, source.to_string());
    }
}
