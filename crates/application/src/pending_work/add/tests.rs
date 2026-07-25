use std::{path::PathBuf, time::SystemTime};

use pwf_domain::{
    handoff::HandoffStatus,
    pending_work::{ProjectName, Timestamp},
};

use super::{
    AddPendingWorkError, AddPendingWorkItem, AddPendingWorkSource, PendingWorkSection,
    ProjectRegistry, plan_title,
};
use crate::{
    HandoffDocument, HandoffDocumentIdentifier, HandoffDocumentScopePresence, HandoffLocation,
    HandoffScope, IndexEntryState,
    handoff::HandoffMutationOk,
    ports::Clock,
    testing::{FailurePoint, InMemoryStore},
};

#[derive(Clone)]
struct FixedClock;

impl Clock for FixedClock {
    fn today(&self) -> Timestamp {
        Timestamp::new("2026-07-26")
    }
}

fn registry(repo: Option<&str>) -> ProjectRegistry {
    ProjectRegistry::new(vec![(
        ProjectName::try_new("pwf").unwrap(),
        repo.map(str::to_string),
        Some("PWF".to_string()),
    )])
}

fn command(section: Option<&str>) -> AddPendingWorkItem {
    AddPendingWorkItem {
        project_identifier: Some("pwf".to_string()),
        source: Some(AddPendingWorkSource::Prompt {
            prompt: "do the thing".to_string(),
            title: Some("ship it".to_string()),
        }),
        date: Some("2026-07-15".to_string()),
        section: section.and_then(PendingWorkSection::from_name),
        prerequisites: Vec::new(),
        effort: None,
        tags: Vec::new(),
    }
}

#[test]
fn add_inserts_record_and_open_index_entry() {
    let store = InMemoryStore::default().with_prefix("pwf", "PWF");

    let added = super::execute(
        &command(None),
        &store,
        &registry(Some("/repo/pwf")),
        &FixedClock,
    )
    .unwrap();

    assert_eq!(added.id, "PWF-0001");
    assert_eq!(added.project, "pwf");
    assert_eq!(added.title, "ship it");
    assert!(!added.title_normalized);
    assert_eq!(added.handoff, HandoffMutationOk::NotLinked);
    assert_eq!(added.created_section, None);
    let entries = store.entries("pwf");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].id.as_ref(), "PWF-0001");
    assert_eq!(entries[0].state, IndexEntryState::Open);
    assert_eq!(store.items("pwf").len(), 1);
    assert_eq!(
        store.items("pwf")[0].created,
        Some(Timestamp::new("2026-07-15"))
    );
}

#[test]
fn add_explicit_prompt_title_is_normalized_once() {
    let store = InMemoryStore::default().with_prefix("pwf", "PWF");
    let mut command = command(None);
    command.source = Some(AddPendingWorkSource::Prompt {
        prompt: "do the thing".to_string(),
        title: Some("fix # metadata".to_string()),
    });

    let added =
        super::execute(&command, &store, &registry(Some("/repo/pwf")), &FixedClock).unwrap();

    assert_eq!(added.title, "fix  metadata");
    assert_eq!(store.items("pwf")[0].title, "fix  metadata");
}

#[test]
fn add_inferred_prompt_title_is_normalized_once() {
    let store = InMemoryStore::default().with_prefix("pwf", "PWF");
    let mut command = command(None);
    command.source = Some(AddPendingWorkSource::Prompt {
        prompt: "fix # metadata".to_string(),
        title: None,
    });

    let added =
        super::execute(&command, &store, &registry(Some("/repo/pwf")), &FixedClock).unwrap();

    assert_eq!(added.title, "fix  metadata");
    assert_eq!(store.items("pwf")[0].title, "fix  metadata");
}

#[test]
fn add_uses_clock_date_when_no_date_is_explicit() {
    let store = InMemoryStore::default().with_prefix("pwf", "PWF");
    let mut command = command(None);
    command.date = None;

    super::execute(&command, &store, &registry(Some("/repo/pwf")), &FixedClock).unwrap();

    assert_eq!(
        store.items("pwf")[0].created,
        Some(Timestamp::new("2026-07-26"))
    );
}

#[test]
fn add_reports_created_section_only_when_region_absent() {
    let store = InMemoryStore::default().with_prefix("pwf", "PWF");

    let added = super::execute(
        &command(Some("Human")),
        &store,
        &registry(Some("/repo/pwf")),
        &FixedClock,
    )
    .unwrap();

    assert_eq!(added.created_section.as_deref(), Some("Human"));
}

#[test]
fn pending_work_section_maps_cli_values_to_canonical_labels() {
    assert_eq!(
        PendingWorkSection::from_name("  LOW-PRIO "),
        Some(PendingWorkSection::LowPriority)
    );
    assert_eq!(PendingWorkSection::LowPriority.as_str(), "Low-prio");
    assert_eq!(PendingWorkSection::from_name("unknown"), None);
}

#[test]
fn plan_title_preserves_legacy_separator_whitespace_and_unicode_rules() {
    assert_eq!(
        plan_title(
            "DÉJÀ--  vu",
            "docs/plans/2026-01-01-MAÑANA__plan--cleanup.md"
        ),
        "déjà vu mañana cleanup"
    );
    assert_eq!(
        plan_title("glep---shimeji", "docs/plans/2026-01-01-plan.md"),
        "glep shimeji plan"
    );
}

#[test]
fn add_plan_normalizes_yaml_significant_filename_title() {
    let store = InMemoryStore::default().with_prefix("pwf", "PWF");
    let mut command = command(None);
    command.source = Some(AddPendingWorkSource::Plan {
        path: "docs/plans/2026-07-15-fix-#-metadata.md".to_string(),
    });

    let added =
        super::execute(&command, &store, &registry(Some("/repo/pwf")), &FixedClock).unwrap();

    assert_eq!(added.title, "pwf fix  metadata");
    assert_eq!(store.items("pwf")[0].title, "pwf fix  metadata");
}

#[test]
fn add_does_not_report_created_section_for_existing_empty_region() {
    let store = InMemoryStore::default()
        .with_prefix("pwf", "PWF")
        .with_sections("pwf", &["Human"]);

    let added = super::execute(
        &command(Some("Human")),
        &store,
        &registry(Some("/repo/pwf")),
        &FixedClock,
    )
    .unwrap();

    assert_eq!(added.created_section, None);
}

#[test]
fn add_rejects_project_without_directory_source() {
    let store = InMemoryStore::default().with_prefix("pwf", "PWF");

    for registry in [registry(None), registry(Some("  "))] {
        let error = super::execute(&command(None), &store, &registry, &FixedClock).unwrap_err();
        assert!(matches!(
            error,
            AddPendingWorkError::ProjectHasNoDirectorySource { ref project } if project == "pwf"
        ));
        assert_eq!(
            error.to_string(),
            "Project 'pwf' has no directory source; update the managed project record."
        );
    }
}

fn scope(repository_root: &str) -> HandoffScope {
    HandoffScope {
        repository_root: PathBuf::from(repository_root),
    }
}

fn handoff_document(
    repository_root: &str,
    file_name: &str,
    modified_timestamp: SystemTime,
) -> HandoffDocument {
    HandoffDocument {
        identifier: HandoffDocumentIdentifier {
            file_name: file_name.to_string(),
            location: HandoffLocation::Active,
        },
        location: HandoffLocation::Active,
        project: Some(ProjectName::try_new("pwf").unwrap()),
        title: "existing handoff".to_string(),
        status: Some(HandoffStatus::Active),
        created: Some(Timestamp::new("2026-07-14")),
        completed: None,
        pending_work_identifier_raw: None,
        goals_completed: 0,
        goals_total: 1,
        body: "\n# Existing handoff\n".to_string(),
        source: String::new(),
        locator: PathBuf::from(repository_root)
            .join("docs/handoffs")
            .join(file_name),
        modified_timestamp,
    }
}

#[test]
fn add_normalizes_explicit_title_and_creates_linked_handoff() {
    let repository_root = "/repo/pwf";
    let store = InMemoryStore::default().with_prefix("pwf", "PWF");
    let mut command = command(None);
    command.source = Some(AddPendingWorkSource::Prompt {
        prompt: "do the thing".to_string(),
        title: Some("Ship: It".to_string()),
    });
    command.tags = vec!["handoff".to_string()];

    let added = super::execute(
        &command,
        &store,
        &registry(Some(repository_root)),
        &FixedClock,
    )
    .unwrap();

    assert_eq!(added.title, "ship; it");
    assert!(added.title_normalized);
    assert_eq!(
        added.handoff,
        HandoffMutationOk::Created {
            path: PathBuf::from(repository_root).join("docs/handoffs/2026-07-15-ship-it.md")
        }
    );
    let documents = store.handoff_documents(&scope(repository_root));
    assert_eq!(documents.len(), 1);
    assert_eq!(
        documents[0].pending_work_identifier_raw.as_deref(),
        Some("PWF-0001")
    );
    assert!(
        store
            .handoff_ledger(&scope(repository_root))
            .unwrap()
            .source
            .contains("PWF-0001")
    );
}

#[test]
fn add_handoff_destination_collision_preflights_before_pending_work_insert() {
    let repository_root = "/repo/pwf";
    let store = InMemoryStore::default()
        .with_prefix("pwf", "PWF")
        .with_handoff_documents(
            scope(repository_root),
            vec![handoff_document(
                repository_root,
                "2026-07-15-ship-it.md",
                SystemTime::UNIX_EPOCH,
            )],
        );
    let mut command = command(None);
    command.tags = vec!["handoff".to_string()];

    let error = super::execute(
        &command,
        &store,
        &registry(Some(repository_root)),
        &FixedClock,
    )
    .unwrap_err();

    assert!(matches!(error, AddPendingWorkError::HandoffPreflight(_)));
    assert!(store.items("pwf").is_empty());
    assert_eq!(store.handoff_documents(&scope(repository_root)).len(), 1);
}

#[test]
fn add_ledger_failure_reports_post_pending_work_phase_and_removes_scaffold() {
    let repository_root = "/repo/pwf";
    let store = InMemoryStore::default()
        .with_prefix("pwf", "PWF")
        .with_failure(FailurePoint::LedgerInsert);
    let mut command = command(Some("Human"));
    command.source = Some(AddPendingWorkSource::Prompt {
        prompt: "do the thing".to_string(),
        title: Some("Ship: It".to_string()),
    });
    command.tags = vec!["handoff".to_string()];

    let error = super::execute(
        &command,
        &store,
        &registry(Some(repository_root)),
        &FixedClock,
    )
    .unwrap_err();

    let AddPendingWorkError::HandoffAfterPendingWork {
        pending_work_identifier,
        diagnostics,
        source,
    } = error
    else {
        panic!("expected post-pending-work handoff failure");
    };
    assert_eq!(pending_work_identifier.as_ref(), "PWF-0001");
    assert_eq!(diagnostics.project, "pwf");
    assert_eq!(diagnostics.created_section.as_deref(), Some("Human"));
    assert!(diagnostics.title_normalized);
    assert!(matches!(
        source,
        crate::handoff::HandoffError::RebuildLedger { .. }
    ));
    assert_eq!(store.items("pwf").len(), 1);
    assert!(
        store.handoff_documents(&scope(repository_root)).is_empty(),
        "failed ledger rebuild must compensate the scaffold"
    );
}

#[test]
fn add_newest_handoff_selects_by_modified_timestamp_without_scaffolding() {
    let repository_root = "/repo/pwf";
    let store = InMemoryStore::default()
        .with_prefix("pwf", "PWF")
        .with_handoff_documents(
            scope(repository_root),
            vec![
                handoff_document(repository_root, "2026-07-14-old.md", SystemTime::UNIX_EPOCH),
                handoff_document(
                    repository_root,
                    "2026-07-15-api-cleanup.md",
                    SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1),
                ),
            ],
        );
    let mut command = command(None);
    command.source = Some(AddPendingWorkSource::NewestHandoff);
    command.tags = vec!["handoff".to_string()];

    let added = super::execute(
        &command,
        &store,
        &registry(Some(repository_root)),
        &FixedClock,
    )
    .unwrap();

    assert_eq!(added.title, "continue api cleanup");
    assert_eq!(added.handoff, HandoffMutationOk::NotLinked);
    assert_eq!(store.handoff_documents(&scope(repository_root)).len(), 2);
    assert_eq!(
        store.items("pwf")[0].body,
        "Continue the handoff at @docs/handoffs/2026-07-15-api-cleanup.md."
    );
}

#[test]
fn add_newest_handoff_normalizes_yaml_significant_filename_title() {
    let repository_root = "/repo/pwf";
    let store = InMemoryStore::default()
        .with_prefix("pwf", "PWF")
        .with_handoff_documents(
            scope(repository_root),
            vec![handoff_document(
                repository_root,
                "2026-07-15-fix-#-metadata.md",
                SystemTime::UNIX_EPOCH,
            )],
        );
    let mut command = command(None);
    command.source = Some(AddPendingWorkSource::NewestHandoff);

    let added = super::execute(
        &command,
        &store,
        &registry(Some(repository_root)),
        &FixedClock,
    )
    .unwrap();

    assert_eq!(added.title, "continue fix  metadata");
    assert_eq!(store.items("pwf")[0].title, "continue fix  metadata");
}

#[test]
fn add_newest_handoff_distinguishes_missing_directory_from_empty_directory() {
    let repository_root = "/repo/pwf";
    let scope = scope(repository_root);
    let missing = InMemoryStore::default()
        .with_prefix("pwf", "PWF")
        .with_handoff_scope_presence(
            scope.clone(),
            HandoffDocumentScopePresence::HandoffDirectoryMissing,
        );
    let mut command = command(None);
    command.source = Some(AddPendingWorkSource::NewestHandoff);

    let error = super::execute(
        &command,
        &missing,
        &registry(Some(repository_root)),
        &FixedClock,
    )
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "No handoff directory found for project at /repo/pwf/docs/handoffs."
    );

    let empty = InMemoryStore::default()
        .with_prefix("pwf", "PWF")
        .with_handoff_scope_presence(scope, HandoffDocumentScopePresence::Present);
    let error = super::execute(
        &command,
        &empty,
        &registry(Some(repository_root)),
        &FixedClock,
    )
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "No handoff Markdown files found in /repo/pwf/docs/handoffs."
    );
}

#[test]
fn add_prerequisite_error_precedes_missing_project_and_source() {
    let store = InMemoryStore::default().with_prefix("pwf", "PWF");
    let command = AddPendingWorkItem {
        project_identifier: None,
        source: None,
        date: Some("2026-07-15".to_string()),
        section: None,
        prerequisites: vec!["PWF-99999".to_string()],
        effort: None,
        tags: Vec::new(),
    };

    let error =
        super::execute(&command, &store, &registry(Some("/repo/pwf")), &FixedClock).unwrap_err();

    assert_eq!(error.to_string(), "Invalid --prereq id: PWF-99999.");
    assert!(store.items("pwf").is_empty());
}

#[test]
fn add_blank_prompt_precedes_unknown_project_resolution() {
    let store = InMemoryStore::default().with_prefix("pwf", "PWF");
    let command = AddPendingWorkItem {
        project_identifier: Some("unknown".to_string()),
        source: Some(AddPendingWorkSource::Prompt {
            prompt: " \t".to_string(),
            title: None,
        }),
        date: Some("2026-07-15".to_string()),
        section: None,
        prerequisites: Vec::new(),
        effort: None,
        tags: Vec::new(),
    };

    let error =
        super::execute(&command, &store, &registry(Some("/repo/pwf")), &FixedClock).unwrap_err();

    assert!(matches!(error, AddPendingWorkError::Usage));
    assert_eq!(error.to_string(), "Use: pwf add <project> \"<prompt>\"");
}
