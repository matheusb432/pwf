use pwf_models::pending_work::{ProjectName, TaskTitle, Timestamp};

use super::{AddPendingWorkError, AddPendingWorkItem, ProjectRegistry, plan_title};
use crate::{
    ports::{clock::Clock, pending_work_record::IndexEntryState},
    testing::InMemoryStore,
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

fn task_title(raw: &str) -> TaskTitle {
    TaskTitle::try_new(raw).unwrap()
}

fn command(section: Option<&str>) -> AddPendingWorkItem {
    AddPendingWorkItem {
        project_identifier: Some("pwf".to_string()),
        prompt: "do the thing".to_string(),
        continue_path: None,
        title: Some(task_title("ship it")),
        date: Some("2026-07-15".to_string()),
        section: section.map(str::to_string),
        human: false,
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
fn add_forwards_an_explicit_task_title() {
    let store = InMemoryStore::default().with_prefix("pwf", "PWF");
    let mut command = command(None);
    command.title = Some(task_title("fix # metadata"));

    let added =
        super::execute(&command, &store, &registry(Some("/repo/pwf")), &FixedClock).unwrap();

    assert_eq!(added.title, "fix  metadata");
    assert_eq!(store.items("pwf")[0].title, "fix  metadata");
}

#[test]
fn add_inferred_prompt_title_is_normalized_once() {
    let store = InMemoryStore::default().with_prefix("pwf", "PWF");
    let mut command = command(None);
    command.prompt = "fix # metadata".to_string();
    command.title = None;

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
fn plan_title_preserves_legacy_separator_whitespace_and_unicode_rules() {
    assert_eq!(
        plan_title(
            "DÉJÀ--  vu",
            "docs/plans/2026-01-01-MAÑANA__plan--cleanup.md"
        ),
        "déjà vu mañana cleanup"
    );
    assert_eq!(
        plan_title("foo---bar", "docs/plans/2026-01-01-plan.md"),
        "foo bar plan"
    );
}

#[test]
fn add_plan_normalizes_yaml_significant_filename_title() {
    let store = InMemoryStore::default().with_prefix("pwf", "PWF");
    let mut command = command(None);
    command.continue_path = Some("docs/plans/2026-07-15-fix-#-metadata.md".to_string());

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

#[test]
fn add_prerequisite_error_precedes_missing_project_and_source() {
    let store = InMemoryStore::default().with_prefix("pwf", "PWF");
    let command = AddPendingWorkItem {
        project_identifier: None,
        prompt: String::new(),
        continue_path: None,
        title: None,
        date: Some("2026-07-15".to_string()),
        section: None,
        human: false,
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
        prompt: " \t".to_string(),
        continue_path: None,
        title: None,
        date: Some("2026-07-15".to_string()),
        section: None,
        human: false,
        prerequisites: Vec::new(),
        effort: None,
        tags: Vec::new(),
    };

    let error =
        super::execute(&command, &store, &registry(Some("/repo/pwf")), &FixedClock).unwrap_err();

    assert!(matches!(error, AddPendingWorkError::Usage));
    assert_eq!(error.to_string(), "Use: pwf add <project> \"<prompt>\"");
}

#[test]
fn explicit_section_wins_over_human_shorthand() {
    let store = InMemoryStore::default().with_prefix("pwf", "PWF");
    let mut command = command(Some("future"));
    command.human = true;

    let added =
        super::execute(&command, &store, &registry(Some("/repo/pwf")), &FixedClock).unwrap();

    assert_eq!(added.created_section.as_deref(), Some("Future"));
}

#[test]
fn invalid_section_is_rejected_before_mutation() {
    let store = InMemoryStore::default().with_prefix("pwf", "PWF");
    let command = command(Some("someday"));

    let error =
        super::execute(&command, &store, &registry(Some("/repo/pwf")), &FixedClock).unwrap_err();

    assert!(matches!(
        error,
        AddPendingWorkError::InvalidSection { ref value } if value == "someday"
    ));
    assert!(store.items("pwf").is_empty());
}
