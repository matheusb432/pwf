use std::{assert_matches, path::PathBuf};

use pwf_application::pending_work::{
    AddedItem, ListResult, OrderDirection, OrderField, OrderSpec, PendingWorkItemView,
    ProjectRegistry, ProjectResolutionError, RemovedItem, UpdatePendingWorkItemOk, note_body,
};
use pwf_domain::pending_work::{ProjectName, WorkItemStatus};

fn project(name: &str) -> ProjectName {
    ProjectName::try_new(name).unwrap()
}

fn registry() -> ProjectRegistry {
    ProjectRegistry::new([
        (
            project("pwf"),
            Some("/repo/pwf".to_string()),
            Some("PWF".to_string()),
        ),
        (
            project("alpha"),
            Some(r"C:\repo\alpha\".to_string()),
            Some("DUP".to_string()),
        ),
        (
            project("beta"),
            Some("/repo/beta".to_string()),
            Some("DUP".to_string()),
        ),
    ])
}

#[test]
fn project_registry_resolves_name_before_prefix_and_reports_structured_failures() {
    let registry = registry();

    assert_eq!(registry.resolve("pwf").unwrap().as_ref(), "pwf");
    assert_eq!(registry.resolve("PWF").unwrap().as_ref(), "pwf");
    assert_matches!(
        registry.resolve("pw"),
        Err(ProjectResolutionError::Unknown {
            ref identifier,
            ref known,
        }) if identifier == "pw"
            && known == &vec!["alpha".to_string(), "beta".to_string(), "pwf".to_string()]
    );
    assert_matches!(
        registry.resolve("dup"),
        Err(ProjectResolutionError::Ambiguous {
            ref identifier,
            ref matches,
        }) if identifier == "dup"
            && matches == &vec!["alpha".to_string(), "beta".to_string()]
    );
}

#[test]
fn project_registry_stops_at_the_first_non_empty_resolution_tier() {
    let registry = ProjectRegistry::new([
        (
            project("PWF"),
            Some("/repo/upper".to_string()),
            Some("UPR".to_string()),
        ),
        (
            project("pwf"),
            Some("/repo/lower".to_string()),
            Some("PWF".to_string()),
        ),
    ]);

    assert_eq!(registry.resolve("pwf").unwrap().as_ref(), "pwf");
    assert_matches!(
        registry.resolve("PwF"),
        Err(ProjectResolutionError::Ambiguous { ref matches, .. })
            if matches == &vec!["PWF".to_string(), "pwf".to_string()]
    );
}

#[test]
fn project_registry_matches_repositories_after_only_contract_normalization() {
    let registry = registry();

    assert_eq!(
        registry
            .project_for_repository("/REPO/PWF/")
            .unwrap()
            .as_ref(),
        "pwf"
    );
    assert_eq!(
        registry
            .project_for_repository("c:/REPO/alpha")
            .unwrap()
            .as_ref(),
        "alpha"
    );
    assert_matches!(
        registry.project_for_repository("/repo/./pwf"),
        Err(ProjectResolutionError::Unknown { .. })
    );
}

#[test]
fn project_registry_selects_the_first_project_for_duplicate_repository_mappings() {
    let registry = ProjectRegistry::new([
        (
            project("alpha"),
            Some("/repo/shared".to_string()),
            Some("ALP".to_string()),
        ),
        (
            project("beta"),
            Some(r"\REPO\SHARED\".to_string()),
            Some("BET".to_string()),
        ),
    ]);

    assert_eq!(
        registry
            .project_for_repository("/repo/shared/")
            .unwrap()
            .as_ref(),
        "alpha"
    );
}

#[test]
fn application_modules_own_pending_work_use_case_values() {
    fn assert_type<T>() {}

    assert_type::<AddedItem>();
    assert_type::<RemovedItem>();
    assert_type::<UpdatePendingWorkItemOk>();
    assert_type::<PendingWorkItemView>();
    assert_type::<ListResult>();
    assert_type::<OrderField>();
    assert_type::<OrderDirection>();
    assert_type::<OrderSpec>();
    assert_eq!(
        OrderSpec::default(),
        OrderSpec {
            field: OrderField::Created,
            direction: OrderDirection::Desc,
        }
    );
    assert_eq!(note_body("TODOé"), "TODOé");

    let _ = AddedItem {
        id: "PWF-0001".to_string(),
        project: "pwf".to_string(),
        title: "task".to_string(),
        note_path: PathBuf::from("/repo/PWF-0001.md"),
        created_section: None,
        title_normalized: false,
        handoff: pwf_application::handoff::HandoffMutationOk::NotLinked,
    };
    let _ = PendingWorkItemView {
        id: "PWF-0001".to_string(),
        project: "pwf".to_string(),
        status: WorkItemStatus::Active,
        session: "task".to_string(),
        prompt: "task".to_string(),
        repo: Some("/repo/pwf".to_string()),
        note: "PWF-0001.md".to_string(),
        item_file: Some("/repo/PWF-0001.md".to_string()),
        line: 1,
        format: "file".to_string(),
        launchable: true,
        needs_prompt: false,
        issues: Vec::new(),
        section: None,
        prereq: None,
        prerequisite_statuses: Vec::new(),
        effort: None,
        tags: None,
        created: Some("2026-07-19".to_string()),
    };
}
