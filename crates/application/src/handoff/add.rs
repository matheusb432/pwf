use std::{error::Error, path::PathBuf};

use pwf_domain::{
    handoff::{continuation_title, slug},
    pending_work::{HANDOFF_TAG, Tags, Timestamp, WorkItemId},
};

use super::{
    ledger, lifecycle,
    ports::{AllocatePendingWork, PendingWorkAllocatorClient},
};
use crate::{
    AppRecordStore, HandoffDocument, HandoffDocumentIdentifier, HandoffDocumentStore,
    HandoffLedger, HandoffLocation, HandoffPatch, HandoffScope, IndexEntry, IndexSection,
    NewHandoffDocument, NewItem, PendingWorkItem,
    pending_work::{ProjectRegistry, ProjectResolutionError, store_util},
};

/// Selects typed in-process allocation or the legacy external CLI protocol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandoffAllocation {
    /// Creates the linked item through the shared application primitive.
    InProcess,
    /// Invokes the configured allocator.
    External,
}

/// Requests one linked handoff and pending-work item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddHandoff {
    /// Repository receiving the handoff document.
    pub scope: HandoffScope,
    /// Handoff heading.
    pub title: String,
    /// Optional caller-selected file slug.
    pub slug: Option<String>,
    /// Authored creation date.
    pub created: String,
    /// Pending-work allocation boundary.
    pub allocation: HandoffAllocation,
}

/// Identifies all records created by a successful handoff add.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddedHandoff {
    /// Linked pending-work identifier.
    pub pending_work_identifier: WorkItemId,
    /// Created handoff document path.
    pub handoff_path: PathBuf,
    /// Rebuilt ledger path.
    pub ledger_path: PathBuf,
}

/// Reports the exact phase in which handoff creation failed.
#[derive(Debug, thiserror::Error)]
pub enum AddHandoffError {
    /// The repository is not mapped to one managed project.
    #[error("{source}")]
    ProjectResolution {
        /// Managed-project lookup failure.
        #[source]
        source: ProjectResolutionError,
    },
    /// The requested dated handoff file already exists.
    #[error("Handoff already exists: {}", path.display())]
    DestinationExists {
        /// Colliding active handoff path.
        path: PathBuf,
    },
    /// Storage failed before a pending-work identifier was allocated.
    #[error("{source}")]
    StoreBeforeAllocation {
        /// Concrete adapter failure.
        #[source]
        source: Box<dyn Error + Send + Sync>,
    },
    /// The typed or external pending-work allocator failed.
    #[error("{source}")]
    AllocationFailed {
        /// Concrete allocator or pending-work store failure.
        #[source]
        source: Box<dyn Error + Send + Sync>,
    },
    /// Allocation succeeded but the handoff could not be linked.
    #[error("{source}")]
    LinkAfterAllocation {
        /// Identifier already allocated by the successful phase.
        pending_work_identifier: WorkItemId,
        /// Provisional handoff requiring recovery.
        handoff_path: PathBuf,
        /// Concrete adapter failure.
        #[source]
        source: Box<dyn Error + Send + Sync>,
    },
    /// Allocation and linking succeeded but ledger replacement failed.
    #[error("{source}")]
    LedgerAfterAllocation {
        /// Identifier already allocated by the successful phase.
        pending_work_identifier: WorkItemId,
        /// Linked handoff requiring recovery.
        handoff_path: PathBuf,
        /// Concrete adapter failure.
        #[source]
        source: Box<dyn Error + Send + Sync>,
    },
}

/// Creates, links, and indexes one handoff through application record ports.
///
/// # Errors
///
/// Returns [`AddHandoffError`] when project resolution, collision preflight, persistence,
/// allocation, linking, or ledger replacement fails.
#[cqrsy::command]
pub fn execute<S, C>(
    command: AddHandoff,
    store: &S,
    projects: &ProjectRegistry,
    allocator: &C,
) -> Result<AddedHandoff, AddHandoffError>
where
    S: AppRecordStore<PendingWorkItem>
        + AppRecordStore<IndexEntry>
        + AppRecordStore<IndexSection>
        + HandoffDocumentStore
        + AppRecordStore<HandoffLedger>,
    C: PendingWorkAllocatorClient,
{
    let project = projects
        .project_for_repository(&command.scope.repository_root)
        .cloned()
        .map_err(|source| AddHandoffError::ProjectResolution { source })?;
    let created = Timestamp::new(command.created);
    let slug = slug(command.slug.as_deref().unwrap_or(&command.title));
    let mut file_name = created.as_str().to_string();
    file_name.push('-');
    file_name.push_str(&slug);
    file_name.push_str(".md");
    let identifier = HandoffDocumentIdentifier {
        file_name: file_name.clone(),
        location: HandoffLocation::Active,
    };
    let handoff_path = command
        .scope
        .repository_root
        .join("docs")
        .join("handoffs")
        .join(&file_name);
    if store
        .document_exists(&command.scope, &identifier)
        .map_err(|error| AddHandoffError::StoreBeforeAllocation {
            source: Box::new(error),
        })?
    {
        return Err(AddHandoffError::DestinationExists { path: handoff_path });
    }

    let body = lifecycle::handoff_body(&command.title);
    let provisional = <S as AppRecordStore<HandoffDocument>>::insert(
        store,
        &command.scope,
        NewHandoffDocument {
            file_name: file_name.clone(),
            project: project.clone(),
            title: command.title,
            created: created.clone(),
            body,
            pending_work_identifier: None,
        },
    )
    .map_err(|error| AddHandoffError::StoreBeforeAllocation {
        source: Box::new(error),
    })?;
    let handoff_path = provisional.locator;

    let pending_work_identifier = match &command.allocation {
        HandoffAllocation::InProcess => allocate_in_process(store, &project, &file_name, &created),
        HandoffAllocation::External => allocator
            .allocate(&AllocatePendingWork {
                created: created.clone(),
                project: project.clone(),
            })
            .map_err(|error| -> Box<dyn Error + Send + Sync> { Box::new(error) }),
    };
    let pending_work_identifier = match pending_work_identifier {
        Ok(identifier) => identifier,
        Err(source) => {
            let _ =
                <S as AppRecordStore<HandoffDocument>>::delete(store, &command.scope, &identifier);
            return Err(AddHandoffError::AllocationFailed { source });
        }
    };

    <S as AppRecordStore<HandoffDocument>>::update(
        store,
        &command.scope,
        &identifier,
        HandoffPatch {
            pending_work_identifier: Some(pending_work_identifier.clone()),
            ..HandoffPatch::default()
        },
    )
    .map_err(|error| AddHandoffError::LinkAfterAllocation {
        pending_work_identifier: pending_work_identifier.clone(),
        handoff_path: handoff_path.clone(),
        source: Box::new(error),
    })?;

    let ledger = ledger::rebuild(&command.scope, store).map_err(|error| {
        AddHandoffError::LedgerAfterAllocation {
            pending_work_identifier: pending_work_identifier.clone(),
            handoff_path: handoff_path.clone(),
            source: Box::new(error),
        }
    })?;
    Ok(AddedHandoff {
        pending_work_identifier,
        handoff_path,
        ledger_path: ledger.locator,
    })
}

fn allocate_in_process<S>(
    store: &S,
    project: &pwf_domain::pending_work::ProjectName,
    file_name: &str,
    created: &Timestamp,
) -> Result<WorkItemId, Box<dyn Error + Send + Sync>>
where
    S: AppRecordStore<PendingWorkItem> + AppRecordStore<IndexEntry> + AppRecordStore<IndexSection>,
{
    let mut relative_path = String::from("docs/handoffs/");
    relative_path.push_str(file_name);
    let mut prompt = String::from("Continue the handoff at @");
    prompt.push_str(&relative_path);
    prompt.push('.');
    let created = store_util::create_item(
        store,
        project,
        NewItem {
            prompt,
            title: Some(continuation_title(file_name)),
            created: created.clone(),
            section: None,
            prereq: None,
            effort: None,
            tags: Some(
                Tags::parse_values(&[HANDOFF_TAG.to_string()])
                    .expect("the handoff tag constant is valid"),
            ),
        },
    )?;
    Ok(created
        .record
        .id
        .as_item()
        .expect("inserted record carries a canonical id")
        .clone())
}

#[cfg(test)]
mod failure_tests;

#[cfg(test)]
mod tests {
    use std::{
        convert::Infallible,
        path::PathBuf,
        sync::{Arc, Mutex},
    };

    use pwf_domain::pending_work::{ProjectName, Timestamp, WorkItemId};

    use super::{AddHandoff, AddHandoffError, HandoffAllocation, execute};
    use crate::{
        AppRecordStore, HandoffDocument, NewHandoffDocument,
        handoff::ports::{AllocatePendingWork, PendingWorkAllocatorClient},
        pending_work::ProjectRegistry,
        ports::{HandoffScope, RecordId},
        testing::InMemoryStore,
    };

    #[derive(Clone)]
    struct RejectingAllocator;

    impl PendingWorkAllocatorClient for RejectingAllocator {
        type Error = Infallible;

        fn allocate(&self, _request: &AllocatePendingWork) -> Result<WorkItemId, Self::Error> {
            panic!("the in-process allocation path must not invoke the process client")
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
    #[error("allocator rejected the request")]
    struct AllocatorError;

    #[derive(Clone)]
    struct RecordingAllocator {
        request: Arc<Mutex<Option<AllocatePendingWork>>>,
        result: Result<WorkItemId, AllocatorError>,
    }

    impl PendingWorkAllocatorClient for RecordingAllocator {
        type Error = AllocatorError;

        fn allocate(&self, request: &AllocatePendingWork) -> Result<WorkItemId, Self::Error> {
            *self.request.lock().unwrap() = Some(request.clone());
            self.result.clone()
        }
    }

    fn project_registry(repository_root: &str) -> ProjectRegistry {
        ProjectRegistry::new([(
            ProjectName::try_new("test-project").unwrap(),
            Some(repository_root.to_string()),
            Some("TST".to_string()),
        )])
    }

    #[test]
    fn in_process_add_inserts_links_and_reconciles_records() {
        let repository_root = PathBuf::from("/repo/test-project");
        let scope = HandoffScope {
            repository_root: repository_root.clone(),
        };
        let store = InMemoryStore::default().with_prefix("test-project", "TST");

        let added = execute(
            AddHandoff {
                scope: scope.clone(),
                title: "Managed Flow".to_string(),
                slug: Some("managed-flow".to_string()),
                created: "2026-01-01".to_string(),
                allocation: HandoffAllocation::InProcess,
            },
            &store,
            &project_registry(&repository_root.to_string_lossy()),
            &RejectingAllocator,
        )
        .unwrap();

        assert_eq!(added.pending_work_identifier.as_ref(), "TST-0001");
        assert_eq!(
            added.handoff_path,
            repository_root.join("docs/handoffs/2026-01-01-managed-flow.md")
        );
        assert_eq!(
            added.ledger_path,
            repository_root.join("docs/handoffs/LEDGER.md")
        );

        let documents = store.handoff_documents(&scope);
        assert_eq!(documents.len(), 1);
        assert_eq!(
            documents[0].pending_work_identifier_raw.as_deref(),
            Some("TST-0001")
        );

        let items = store.items("test-project");
        assert_eq!(items.len(), 1);
        assert_eq!(
            items[0].id,
            RecordId::Item(WorkItemId::try_new("TST-0001").unwrap())
        );
        assert_eq!(items[0].title, "continue managed flow");
        assert_eq!(
            items[0].body,
            "Continue the handoff at @docs/handoffs/2026-01-01-managed-flow.md."
        );
        assert_eq!(items[0].created, Some(Timestamp::new("2026-01-01")));

        let ledger = store.handoff_ledger(&scope).expect("ledger was rebuilt");
        assert!(ledger.source.contains(
            "| TST-0001 | [Managed Flow](2026-01-01-managed-flow.md) | 0/1 | 2026-01-01 |"
        ));
    }

    #[test]
    fn external_add_calls_the_allocator_with_typed_context_and_links_its_identifier() {
        let repository_root = PathBuf::from("/repo/test-project");
        let scope = HandoffScope {
            repository_root: repository_root.clone(),
        };
        let store = InMemoryStore::default().with_prefix("test-project", "TST");
        let request = Arc::new(Mutex::new(None));
        let allocator = RecordingAllocator {
            request: Arc::clone(&request),
            result: Ok(WorkItemId::try_new("TST-0042").unwrap()),
        };

        let added = execute(
            AddHandoff {
                scope: scope.clone(),
                title: "Managed Flow".to_string(),
                slug: None,
                created: "2026-01-01".to_string(),
                allocation: HandoffAllocation::External,
            },
            &store,
            &project_registry(&repository_root.to_string_lossy()),
            &allocator,
        )
        .unwrap();

        assert_eq!(added.pending_work_identifier.as_ref(), "TST-0042");
        assert_eq!(store.items("test-project"), []);
        let allocated = request.lock().unwrap().clone().unwrap();
        assert_eq!(allocated.created.as_str(), "2026-01-01");
        assert_eq!(allocated.project.as_ref(), "test-project");
        assert_eq!(
            store.handoff_documents(&scope)[0]
                .pending_work_identifier_raw
                .as_deref(),
            Some("TST-0042")
        );
    }

    #[test]
    fn allocator_failure_removes_the_provisional_document() {
        let repository_root = PathBuf::from("/repo/test-project");
        let scope = HandoffScope {
            repository_root: repository_root.clone(),
        };
        let store = InMemoryStore::default().with_prefix("test-project", "TST");
        let allocator = RecordingAllocator {
            request: Arc::new(Mutex::new(None)),
            result: Err(AllocatorError),
        };

        let error = execute(
            AddHandoff {
                scope: scope.clone(),
                title: "Managed Flow".to_string(),
                slug: None,
                created: "2026-01-01".to_string(),
                allocation: HandoffAllocation::External,
            },
            &store,
            &project_registry(&repository_root.to_string_lossy()),
            &allocator,
        )
        .unwrap_err();

        assert!(matches!(error, AddHandoffError::AllocationFailed { .. }));
        assert!(store.handoff_documents(&scope).is_empty());
        assert!(store.handoff_ledger(&scope).is_none());
    }

    #[test]
    fn destination_collision_fails_before_allocation() {
        let repository_root = PathBuf::from("/repo/test-project");
        let scope = HandoffScope {
            repository_root: repository_root.clone(),
        };
        let store = InMemoryStore::default().with_prefix("test-project", "TST");
        <InMemoryStore as AppRecordStore<HandoffDocument>>::insert(
            &store,
            &scope,
            NewHandoffDocument {
                file_name: "2026-01-01-managed-flow.md".to_string(),
                project: ProjectName::try_new("test-project").unwrap(),
                title: "Existing".to_string(),
                created: Timestamp::new("2026-01-01"),
                body: "\n# Existing\n".to_string(),
                pending_work_identifier: None,
            },
        )
        .unwrap();

        let error = execute(
            AddHandoff {
                scope,
                title: "Managed Flow".to_string(),
                slug: None,
                created: "2026-01-01".to_string(),
                allocation: HandoffAllocation::InProcess,
            },
            &store,
            &project_registry(&repository_root.to_string_lossy()),
            &RejectingAllocator,
        )
        .unwrap_err();

        assert!(matches!(error, AddHandoffError::DestinationExists { .. }));
        assert!(store.items("test-project").is_empty());
    }
}
