use std::{path::PathBuf, time::SystemTime};

use pwf_domain::{
    handoff::HandoffStatus,
    pending_work::{ProjectName, Timestamp, WorkItemId, WorkItemStatus},
};

use super::{
    CloseHandoffAction, PendingHandoffMutation, commit_after_pending_work, handoff_body,
    preflight_close, preflight_delete, preflight_reopen,
};
use crate::{
    HandoffDocument, HandoffDocumentIdentifier, HandoffLocation, HandoffScope, Materialization,
    PendingWorkItem, RecordId,
    handoff::{HandoffError, HandoffMutationOk},
    pending_work::ProjectRegistry,
    testing::{FailurePoint, InMemoryStore},
};

const REPOSITORY_ROOT: &str = "/repo/pwf";
const FILE_NAME: &str = "2026-07-19-managed-flow.md";

fn registry(repository: Option<&str>) -> ProjectRegistry {
    ProjectRegistry::new([(
        ProjectName::try_new("pwf").unwrap(),
        repository.map(str::to_string),
        Some("PWF".to_string()),
    )])
}

fn scope() -> HandoffScope {
    HandoffScope {
        repository_root: PathBuf::from(REPOSITORY_ROOT),
    }
}

fn pending_work(tags: Option<&str>, status: WorkItemStatus) -> PendingWorkItem {
    PendingWorkItem {
        id: RecordId::Item(WorkItemId::try_new("PWF-0001").unwrap()),
        title: "managed flow".to_string(),
        status,
        created: Some(Timestamp::new("2026-07-19")),
        completed: None,
        commits: None,
        tags: tags.map(str::to_string),
        effort: None,
        prereq: None,
        section: None,
        body: "body".to_string(),
        source: "body".to_string(),
        locator: "/notes/pwf/PWF-0001.md".to_string(),
        placement: None,
        materialization: Materialization::NoteFile,
    }
}

fn document(
    location: HandoffLocation,
    status: Option<HandoffStatus>,
    pending_work_identifier: &str,
) -> HandoffDocument {
    let directory = PathBuf::from(REPOSITORY_ROOT).join("docs/handoffs");
    let locator = match location {
        HandoffLocation::Active => directory.join(FILE_NAME),
        HandoffLocation::Archived => directory.join("archived").join(FILE_NAME),
    };
    let status_line = status.map_or_else(
        || "legacy: true\n".to_string(),
        |status| format!("status: {status}\n"),
    );
    let body = "\n# Managed Flow\n\nlegacy spacing  \n".to_string();
    HandoffDocument {
        identifier: HandoffDocumentIdentifier {
            file_name: FILE_NAME.to_string(),
            location,
        },
        location,
        project: Some(ProjectName::try_new("pwf").unwrap()),
        title: "Managed Flow".to_string(),
        status,
        created: Some(Timestamp::new("2026-07-19")),
        completed: (location == HandoffLocation::Archived).then(|| Timestamp::new("2026-07-20")),
        pending_work_identifier_raw: Some(pending_work_identifier.to_string()),
        goals_completed: 0,
        goals_total: 0,
        source: format!(
            "---\n{status_line}project: pwf\ncreated: 2026-07-19\npw: {pending_work_identifier}\n---\n{body}"
        ),
        body,
        locator,
        modified_timestamp: SystemTime::UNIX_EPOCH,
    }
}

fn store(documents: Vec<HandoffDocument>) -> InMemoryStore {
    InMemoryStore::default()
        .with_project(
            "pwf",
            vec![pending_work(Some("[handoff]"), WorkItemStatus::Active)],
        )
        .with_handoff_documents(scope(), documents)
}

#[test]
fn scaffold_body_is_the_exact_shared_template_body() {
    assert_eq!(
        handoff_body("Managed Flow"),
        "\n# Managed Flow\n\n## Goals\n- [ ] <task title> :: <task description>\n\n## Context\n\n## Next steps\n-\n\n<!-- Lifecycle: while active, this is a LIVE document — check off Goals as you finish them.\n     When all Goals are done run `pwf done --id <pw-id>` (closes the task and archives this handoff). Never edit archived/. -->\n"
    );
}

#[test]
fn close_selects_one_active_link_case_insensitively_and_archives_it() {
    let store = store(vec![document(
        HandoffLocation::Active,
        Some(HandoffStatus::Active),
        "pwf-0001",
    )]);
    let pending = preflight_close(
        &store,
        &registry(Some(REPOSITORY_ROOT)),
        "PWF-0001",
        CloseHandoffAction::Done,
        "2026-07-20",
        None,
    )
    .unwrap();

    let outcome = commit_after_pending_work(&store, pending).unwrap();

    assert_eq!(
        outcome,
        HandoffMutationOk::Archived {
            path: PathBuf::from(REPOSITORY_ROOT)
                .join("docs/handoffs/archived")
                .join(FILE_NAME),
        }
    );
    let documents = store.handoff_documents(&scope());
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].location, HandoffLocation::Archived);
    assert_eq!(documents[0].status, Some(HandoffStatus::Done));
    assert_eq!(documents[0].completed, Some(Timestamp::new("2026-07-20")));
}

#[test]
fn close_cancelled_appends_the_exact_reason_spacing() {
    let store = store(vec![document(
        HandoffLocation::Active,
        Some(HandoffStatus::Active),
        "PWF-0001",
    )]);
    let pending = preflight_close(
        &store,
        &registry(Some(REPOSITORY_ROOT)),
        "PWF-0001",
        CloseHandoffAction::Cancelled,
        "2026-07-20",
        Some("obsolete"),
    )
    .unwrap();

    commit_after_pending_work(&store, pending).unwrap();

    let archived = &store.handoff_documents(&scope())[0];
    assert_eq!(
        archived.body,
        "\n# Managed Flow\n\nlegacy spacing\n\n> Cancelled: obsolete\n"
    );
}

#[test]
fn close_already_archived_returns_a_typed_idempotent_mutation() {
    let store = store(vec![document(
        HandoffLocation::Archived,
        Some(HandoffStatus::Done),
        "PWF-0001",
    )]);

    let pending = preflight_close(
        &store,
        &registry(Some(REPOSITORY_ROOT)),
        "PWF-0001",
        CloseHandoffAction::Done,
        "2026-07-20",
        None,
    )
    .unwrap();

    assert!(matches!(
        pending,
        PendingHandoffMutation::AlreadyInTargetState
    ));
    assert_eq!(
        commit_after_pending_work(&store, pending).unwrap(),
        HandoffMutationOk::AlreadyInTargetState
    );
}

#[test]
fn close_duplicate_active_links_are_rejected_before_mutation() {
    let mut second = document(
        HandoffLocation::Active,
        Some(HandoffStatus::Active),
        "PWF-0001",
    );
    second.identifier.file_name = "2026-07-18-other.md".to_string();
    second.locator = PathBuf::from(REPOSITORY_ROOT).join("docs/handoffs/2026-07-18-other.md");
    let store = store(vec![
        document(
            HandoffLocation::Active,
            Some(HandoffStatus::Active),
            "PWF-0001",
        ),
        second,
    ]);

    let error = preflight_close(
        &store,
        &registry(Some(REPOSITORY_ROOT)),
        "PWF-0001",
        CloseHandoffAction::Done,
        "2026-07-20",
        None,
    )
    .unwrap_err();

    assert!(matches!(error, HandoffError::AmbiguousHandoff { .. }));
    assert_eq!(store.handoff_documents(&scope()).len(), 2);
}

#[test]
fn reopen_accepts_a_legacy_archived_document_without_a_valid_status() {
    let store = store(vec![document(HandoffLocation::Archived, None, "PWF-0001")]);
    let pending = preflight_reopen(&store, &registry(Some(REPOSITORY_ROOT)), "PWF-0001").unwrap();

    let outcome = commit_after_pending_work(&store, pending).unwrap();

    assert!(matches!(outcome, HandoffMutationOk::Reopened { .. }));
    let active = &store.handoff_documents(&scope())[0];
    assert_eq!(active.location, HandoffLocation::Active);
    assert_eq!(active.status, Some(HandoffStatus::Active));
    assert_eq!(active.completed, None);
}

#[test]
fn ledger_failure_after_close_restores_the_exact_original_document() {
    let original = document(
        HandoffLocation::Active,
        Some(HandoffStatus::Active),
        "PWF-0001",
    );
    let source = original.source.clone();
    let store = store(vec![original]).with_failure(FailurePoint::LedgerInsert);
    let pending = preflight_close(
        &store,
        &registry(Some(REPOSITORY_ROOT)),
        "PWF-0001",
        CloseHandoffAction::Done,
        "2026-07-20",
        None,
    )
    .unwrap();

    let error = commit_after_pending_work(&store, pending).unwrap_err();

    assert!(matches!(error, HandoffError::RebuildLedger { .. }));
    let documents = store.handoff_documents(&scope());
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].location, HandoffLocation::Active);
    assert_eq!(documents[0].source, source);
}

#[test]
fn ledger_and_restoration_failure_returns_the_primary_ledger_error() {
    let scope = scope();
    let original = document(
        HandoffLocation::Active,
        Some(HandoffStatus::Active),
        "PWF-0001",
    );
    let store = store(vec![original]).with_failures([
        FailurePoint::LedgerInsert,
        FailurePoint::DocumentRestoreMove,
    ]);
    let pending = preflight_close(
        &store,
        &registry(Some("/repo/pwf")),
        "PWF-0001",
        CloseHandoffAction::Done,
        "2026-07-20",
        None,
    )
    .unwrap();

    let error = commit_after_pending_work(&store, pending).unwrap_err();

    assert!(matches!(error, HandoffError::RebuildLedger { .. }));
    assert!(error.to_string().contains("handoff-ledger-insert"));
    assert_eq!(
        store.handoff_documents(&scope)[0].location,
        HandoffLocation::Archived,
        "failed best-effort restoration must not replace the primary ledger error"
    );
}

#[test]
fn delete_ledger_failure_restores_active_without_touching_same_name_archive() {
    let active = document(
        HandoffLocation::Active,
        Some(HandoffStatus::Active),
        "PWF-0001",
    );
    let archived = document(
        HandoffLocation::Archived,
        Some(HandoffStatus::Done),
        "PWF-9999",
    );
    let archived_source = archived.source.clone();
    let store = store(vec![active, archived]).with_failure(FailurePoint::LedgerInsert);
    let pending = preflight_delete(&store, &registry(Some(REPOSITORY_ROOT)), "PWF-0001").unwrap();

    let error = commit_after_pending_work(&store, pending).unwrap_err();

    assert!(matches!(error, HandoffError::RebuildLedger { .. }));
    let documents = store.handoff_documents(&scope());
    assert_eq!(documents.len(), 2);
    assert!(documents.iter().any(|document| {
        document.location == HandoffLocation::Archived && document.source == archived_source
    }));
}

#[test]
fn malformed_tags_fail_before_handoff_document_mutation() {
    let store = InMemoryStore::default().with_project(
        "pwf",
        vec![pending_work(Some("handoff"), WorkItemStatus::Active)],
    );

    let error = preflight_delete(&store, &registry(Some(REPOSITORY_ROOT)), "PWF-0001").unwrap_err();

    assert!(matches!(error, HandoffError::InvalidTags { .. }));
}
