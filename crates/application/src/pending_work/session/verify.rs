//! Verifies pending-work launchability without probing or rendering a provider.

use thiserror::Error;

use super::{Agent, ModelTierCatalog, VerifySessionOk};
use crate::{
    AppDbStore, PendingWorkItem,
    pending_work::{
        find::{FindPendingWorkError, find_open_item},
        project_registry::ProjectRegistry,
        session::{AgentLaunch, model_selection::resolve_model},
    },
};

/// Requests optional pending-work launchability context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifySession {
    pub id: Option<String>,
    pub agent: Agent,
    pub model_override: Option<String>,
}

/// Reports pending-work lookup failures during verification.
#[derive(Debug, Error)]
pub enum VerifySessionError {
    #[error(transparent)]
    Find(#[from] FindPendingWorkError),
}

/// Verifies one open item and prepares its provider-neutral launch.
///
/// # Errors
///
/// Returns [`VerifySessionError`] when the requested item cannot be read. Model-tier failures
/// remain soft issues in [`VerifySessionOk`].
#[cqrsy::query]
pub fn execute(
    query: VerifySession,
    store: &impl AppDbStore<PendingWorkItem>,
    projects: &ProjectRegistry,
    model_tiers: &impl ModelTierCatalog,
) -> Result<VerifySessionOk, VerifySessionError> {
    let Some(id) = query.id else {
        return Ok(VerifySessionOk {
            task_id: None,
            launchable: true,
            issues: Vec::new(),
            launch: None,
        });
    };

    let item = find_open_item(store, projects, &id)?;
    let (model, model_issue) = match query.model_override {
        Some(model) => (Some(model), None),
        None => match resolve_model(model_tiers, query.agent, &item.id, item.effort.as_deref()) {
            Ok(model) => (model, None),
            Err(issue) => (None, Some(issue)),
        },
    };
    let launch = AgentLaunch::new(
        &item,
        super::LaunchDirectives::default(),
        query.agent,
        model,
    );
    let mut issues = item.issues;
    let mut launchable = item.launchable;
    if let Some(issue) = model_issue {
        launchable = false;
        issues.push(issue.to_string());
    }
    Ok(VerifySessionOk {
        task_id: Some(item.id),
        launchable,
        issues,
        launch: Some(launch),
    })
}

#[cfg(test)]
mod tests {
    use std::{error::Error, fmt};

    use pwf_domain::pending_work::{
        EffortTier, ProjectName, Timestamp, WorkItemId, WorkItemStatus,
    };

    use super::{ProjectRegistry, VerifySession, execute};
    use crate::{
        IndexPlacement, Materialization, PendingWorkItem, RecordId,
        pending_work::session::{Agent, ModelTierCatalog, ModelTierLookup},
        testing::InMemoryStore,
    };

    const REPOSITORY: &str = "/repo/pwf";
    const TASK_ID: &str = "PWF-0139";

    #[derive(Debug, Clone)]
    struct CatalogError;

    impl fmt::Display for CatalogError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("catalog unavailable")
        }
    }

    impl Error for CatalogError {}

    #[derive(Debug, Clone)]
    struct BrokenCatalog;

    impl ModelTierCatalog for BrokenCatalog {
        type Error = CatalogError;

        fn tier(&self, _effort: EffortTier) -> Result<ModelTierLookup, Self::Error> {
            Err(CatalogError)
        }
    }

    fn record() -> PendingWorkItem {
        PendingWorkItem {
            id: RecordId::Item(WorkItemId::try_new(TASK_ID).unwrap()),
            title: "application verify".to_string(),
            status: WorkItemStatus::Active,
            created: Some(Timestamp::new("2026-07-15")),
            completed: None,
            commits: None,
            tags: None,
            effort: Some("4".to_string()),
            prereq: None,
            section: None,
            body: "tbd".to_string(),
            source: String::new(),
            locator: format!("/notes/{TASK_ID}.md"),
            placement: Some(IndexPlacement {
                index_path: "/notes/pwf.md".to_string(),
                line: 1,
            }),
            materialization: Materialization::NoteFile,
        }
    }

    #[test]
    fn item_and_model_issues_are_aggregated() {
        let store = InMemoryStore::default().with_project("pwf", vec![record()]);
        let projects = ProjectRegistry::new([(
            ProjectName::try_new("pwf").unwrap(),
            Some(REPOSITORY.to_string()),
            Some("PWF".to_string()),
        )]);

        let outcome = execute(
            VerifySession {
                id: Some(TASK_ID.to_string()),
                agent: Agent::Claude,
                model_override: None,
            },
            &store,
            &projects,
            &BrokenCatalog,
        )
        .unwrap();

        assert!(!outcome.launchable);
        assert!(
            outcome
                .issues
                .iter()
                .any(|issue| issue.contains("placeholder"))
        );
        assert_eq!(
            outcome.issues.last().map(String::as_str),
            Some("catalog unavailable")
        );
    }
}
