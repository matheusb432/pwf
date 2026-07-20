use std::{convert::Infallible, error::Error as _, path::PathBuf};

use pwf_domain::pending_work::{ProjectName, WorkItemId};

use super::{AddHandoff, AddHandoffError, HandoffAllocation, execute};
use crate::{
    AppDbStore, HandoffDocument, HandoffDocumentIdentifier, HandoffDocumentScopePresence,
    HandoffDocumentStore, HandoffLedger, HandoffLedgerIdentifier, HandoffLedgerWrite,
    HandoffLocation, HandoffPatch, HandoffScope, IndexEntry, IndexSection, ItemPatch,
    NewHandoffDocument, NewItem, PendingWorkItem,
    handoff::ports::{AllocatePendingWork, PendingWorkAllocatorClient},
    pending_work::ProjectRegistry,
    testing::{InMemoryStore, InMemoryStoreError},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FailurePoint {
    ReadPreflight,
    UpdateLink,
    ReadLedger,
}

#[derive(Debug, thiserror::Error)]
enum FailureStoreError {
    #[error("sentinel adapter failure")]
    Sentinel,
    #[error(transparent)]
    InMemory(#[from] InMemoryStoreError),
}

#[derive(Clone)]
struct FailureStore {
    inner: InMemoryStore,
    point: FailurePoint,
}

impl FailureStore {
    fn new(point: FailurePoint) -> Self {
        Self {
            inner: InMemoryStore::default(),
            point,
        }
    }
}

impl AppDbStore<HandoffDocument> for FailureStore {
    type Error = FailureStoreError;

    fn get(
        &self,
        scope: &HandoffScope,
        identifier: &HandoffDocumentIdentifier,
    ) -> Result<Option<HandoffDocument>, Self::Error> {
        if self.point == FailurePoint::ReadPreflight {
            return Err(FailureStoreError::Sentinel);
        }
        <InMemoryStore as AppDbStore<HandoffDocument>>::get(&self.inner, scope, identifier)
            .map_err(Into::into)
    }

    fn list(&self, scope: &HandoffScope) -> Result<Vec<HandoffDocument>, Self::Error> {
        if self.point == FailurePoint::ReadLedger {
            return Err(FailureStoreError::Sentinel);
        }
        <InMemoryStore as AppDbStore<HandoffDocument>>::list(&self.inner, scope).map_err(Into::into)
    }

    fn insert(
        &self,
        scope: &HandoffScope,
        new: NewHandoffDocument,
    ) -> Result<HandoffDocument, Self::Error> {
        <InMemoryStore as AppDbStore<HandoffDocument>>::insert(&self.inner, scope, new)
            .map_err(Into::into)
    }

    fn update(
        &self,
        scope: &HandoffScope,
        identifier: &HandoffDocumentIdentifier,
        patch: HandoffPatch,
    ) -> Result<(), Self::Error> {
        if self.point == FailurePoint::UpdateLink {
            return Err(FailureStoreError::Sentinel);
        }
        <InMemoryStore as AppDbStore<HandoffDocument>>::update(
            &self.inner,
            scope,
            identifier,
            patch,
        )
        .map_err(Into::into)
    }

    fn delete(
        &self,
        scope: &HandoffScope,
        identifier: &HandoffDocumentIdentifier,
    ) -> Result<(), Self::Error> {
        <InMemoryStore as AppDbStore<HandoffDocument>>::delete(&self.inner, scope, identifier)
            .map_err(Into::into)
    }
}

impl HandoffDocumentStore for FailureStore {
    fn scope_presence(
        &self,
        scope: &HandoffScope,
    ) -> Result<HandoffDocumentScopePresence, Self::Error> {
        self.inner.scope_presence(scope).map_err(Into::into)
    }

    fn document_exists(
        &self,
        scope: &HandoffScope,
        identifier: &HandoffDocumentIdentifier,
    ) -> Result<bool, Self::Error> {
        if self.point == FailurePoint::ReadPreflight {
            return Err(FailureStoreError::Sentinel);
        }
        self.inner
            .document_exists(scope, identifier)
            .map_err(Into::into)
    }

    fn list_location(
        &self,
        scope: &HandoffScope,
        location: HandoffLocation,
    ) -> Result<Vec<HandoffDocument>, Self::Error> {
        if self.point == FailurePoint::ReadLedger {
            return Err(FailureStoreError::Sentinel);
        }
        self.inner
            .list_location(scope, location)
            .map_err(Into::into)
    }

    fn restore_document_after_move(
        &self,
        scope: &HandoffScope,
        snapshot: &HandoffDocument,
    ) -> Result<(), Self::Error> {
        self.inner
            .restore_document_after_move(scope, snapshot)
            .map_err(Into::into)
    }

    fn restore_document_after_delete(
        &self,
        scope: &HandoffScope,
        snapshot: &HandoffDocument,
    ) -> Result<(), Self::Error> {
        self.inner
            .restore_document_after_delete(scope, snapshot)
            .map_err(Into::into)
    }
}

impl AppDbStore<HandoffLedger> for FailureStore {
    type Error = FailureStoreError;

    fn get(
        &self,
        scope: &HandoffScope,
        identifier: &HandoffLedgerIdentifier,
    ) -> Result<Option<HandoffLedger>, Self::Error> {
        <InMemoryStore as AppDbStore<HandoffLedger>>::get(&self.inner, scope, identifier)
            .map_err(Into::into)
    }

    fn list(&self, scope: &HandoffScope) -> Result<Vec<HandoffLedger>, Self::Error> {
        <InMemoryStore as AppDbStore<HandoffLedger>>::list(&self.inner, scope).map_err(Into::into)
    }

    fn insert(
        &self,
        scope: &HandoffScope,
        new: HandoffLedgerWrite,
    ) -> Result<HandoffLedger, Self::Error> {
        <InMemoryStore as AppDbStore<HandoffLedger>>::insert(&self.inner, scope, new)
            .map_err(Into::into)
    }

    fn update(
        &self,
        scope: &HandoffScope,
        identifier: &HandoffLedgerIdentifier,
        patch: HandoffLedgerWrite,
    ) -> Result<(), Self::Error> {
        <InMemoryStore as AppDbStore<HandoffLedger>>::update(&self.inner, scope, identifier, patch)
            .map_err(Into::into)
    }

    fn delete(
        &self,
        scope: &HandoffScope,
        identifier: &HandoffLedgerIdentifier,
    ) -> Result<(), Self::Error> {
        <InMemoryStore as AppDbStore<HandoffLedger>>::delete(&self.inner, scope, identifier)
            .map_err(Into::into)
    }
}

impl AppDbStore<PendingWorkItem> for FailureStore {
    type Error = FailureStoreError;

    fn get(
        &self,
        scope: &ProjectName,
        identifier: &WorkItemId,
    ) -> Result<Option<PendingWorkItem>, Self::Error> {
        <InMemoryStore as AppDbStore<PendingWorkItem>>::get(&self.inner, scope, identifier)
            .map_err(infallible)
    }

    fn list(&self, scope: &ProjectName) -> Result<Vec<PendingWorkItem>, Self::Error> {
        <InMemoryStore as AppDbStore<PendingWorkItem>>::list(&self.inner, scope).map_err(infallible)
    }

    fn insert(&self, scope: &ProjectName, new: NewItem) -> Result<PendingWorkItem, Self::Error> {
        <InMemoryStore as AppDbStore<PendingWorkItem>>::insert(&self.inner, scope, new)
            .map_err(infallible)
    }

    fn update(
        &self,
        scope: &ProjectName,
        identifier: &WorkItemId,
        patch: ItemPatch,
    ) -> Result<(), Self::Error> {
        <InMemoryStore as AppDbStore<PendingWorkItem>>::update(
            &self.inner,
            scope,
            identifier,
            patch,
        )
        .map_err(infallible)
    }

    fn delete(&self, scope: &ProjectName, identifier: &WorkItemId) -> Result<(), Self::Error> {
        <InMemoryStore as AppDbStore<PendingWorkItem>>::delete(&self.inner, scope, identifier)
            .map_err(infallible)
    }
}

impl AppDbStore<IndexEntry> for FailureStore {
    type Error = FailureStoreError;

    fn get(
        &self,
        scope: &ProjectName,
        identifier: &WorkItemId,
    ) -> Result<Option<IndexEntry>, Self::Error> {
        <InMemoryStore as AppDbStore<IndexEntry>>::get(&self.inner, scope, identifier)
            .map_err(infallible)
    }

    fn list(&self, scope: &ProjectName) -> Result<Vec<IndexEntry>, Self::Error> {
        <InMemoryStore as AppDbStore<IndexEntry>>::list(&self.inner, scope).map_err(infallible)
    }

    fn insert(&self, scope: &ProjectName, new: IndexEntry) -> Result<IndexEntry, Self::Error> {
        <InMemoryStore as AppDbStore<IndexEntry>>::insert(&self.inner, scope, new)
            .map_err(infallible)
    }

    fn update(
        &self,
        scope: &ProjectName,
        identifier: &WorkItemId,
        patch: IndexEntry,
    ) -> Result<(), Self::Error> {
        <InMemoryStore as AppDbStore<IndexEntry>>::update(&self.inner, scope, identifier, patch)
            .map_err(infallible)
    }

    fn delete(&self, scope: &ProjectName, identifier: &WorkItemId) -> Result<(), Self::Error> {
        <InMemoryStore as AppDbStore<IndexEntry>>::delete(&self.inner, scope, identifier)
            .map_err(infallible)
    }
}

impl AppDbStore<IndexSection> for FailureStore {
    type Error = FailureStoreError;

    fn get(
        &self,
        scope: &ProjectName,
        identifier: &String,
    ) -> Result<Option<IndexSection>, Self::Error> {
        <InMemoryStore as AppDbStore<IndexSection>>::get(&self.inner, scope, identifier)
            .map_err(Into::into)
    }

    fn list(&self, scope: &ProjectName) -> Result<Vec<IndexSection>, Self::Error> {
        <InMemoryStore as AppDbStore<IndexSection>>::list(&self.inner, scope).map_err(Into::into)
    }

    fn insert(&self, scope: &ProjectName, new: IndexSection) -> Result<IndexSection, Self::Error> {
        <InMemoryStore as AppDbStore<IndexSection>>::insert(&self.inner, scope, new)
            .map_err(Into::into)
    }

    fn update(
        &self,
        scope: &ProjectName,
        identifier: &String,
        patch: IndexSection,
    ) -> Result<(), Self::Error> {
        <InMemoryStore as AppDbStore<IndexSection>>::update(&self.inner, scope, identifier, patch)
            .map_err(Into::into)
    }

    fn delete(&self, scope: &ProjectName, identifier: &String) -> Result<(), Self::Error> {
        <InMemoryStore as AppDbStore<IndexSection>>::delete(&self.inner, scope, identifier)
            .map_err(Into::into)
    }
}

#[derive(Clone)]
struct SuccessfulAllocator;

impl PendingWorkAllocatorClient for SuccessfulAllocator {
    type Error = Infallible;

    fn allocate(&self, _request: &AllocatePendingWork) -> Result<WorkItemId, Self::Error> {
        Ok(WorkItemId::try_new("TST-0042").expect("valid test identifier"))
    }
}

fn infallible(error: Infallible) -> FailureStoreError {
    match error {}
}

fn failed_add(point: FailurePoint) -> AddHandoffError {
    let repository_root = PathBuf::from("/repo/test-project");
    let projects = ProjectRegistry::new([(
        ProjectName::try_new("test-project").expect("valid test project"),
        Some(repository_root.to_string_lossy().into_owned()),
        Some("TST".to_string()),
    )]);
    execute(
        AddHandoff {
            scope: HandoffScope { repository_root },
            title: "Managed Flow".to_string(),
            slug: None,
            created: "2026-01-01".to_string(),
            allocation: HandoffAllocation::External {
                config_path: PathBuf::from("/tmp/pending-work.json"),
            },
        },
        &FailureStore::new(point),
        &projects,
        &SuccessfulAllocator,
    )
    .expect_err("injected store failure must fail handoff creation")
}

fn expected_handoff_path() -> PathBuf {
    PathBuf::from("/repo/test-project/docs/handoffs/2026-01-01-managed-flow.md")
}

fn assert_sentinel(error: &(dyn std::error::Error + 'static)) {
    assert!(
        matches!(
            error.downcast_ref::<FailureStoreError>(),
            Some(FailureStoreError::Sentinel)
        ),
        "unexpected source: {error:?}"
    );
}

#[test]
fn store_before_allocation_retains_the_adapter_source() {
    let error = failed_add(FailurePoint::ReadPreflight);

    assert!(matches!(
        error,
        AddHandoffError::StoreBeforeAllocation { .. }
    ));
    assert_sentinel(error.source().expect("adapter source"));
}

#[test]
fn link_after_allocation_retains_context_and_the_adapter_source() {
    let error = failed_add(FailurePoint::UpdateLink);

    assert!(matches!(
        &error,
        AddHandoffError::LinkAfterAllocation {
            pending_work_identifier,
            handoff_path,
            ..
        } if pending_work_identifier.as_ref() == "TST-0042"
            && *handoff_path == expected_handoff_path()
    ));
    assert_sentinel(error.source().expect("adapter source"));
}

#[test]
fn ledger_after_allocation_chains_through_rebuild_to_the_adapter_source() {
    let error = failed_add(FailurePoint::ReadLedger);

    assert!(matches!(
        &error,
        AddHandoffError::LedgerAfterAllocation {
            pending_work_identifier,
            handoff_path,
            ..
        } if pending_work_identifier.as_ref() == "TST-0042"
            && *handoff_path == expected_handoff_path()
    ));
    let rebuild = error.source().expect("rebuild source");
    assert_sentinel(rebuild.source().expect("adapter source"));
}
