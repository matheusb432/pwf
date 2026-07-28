//! Verifies pending-work launchability with the selected provider.

use thiserror::Error;

use super::{
    Agent, AgentLaunch, ClaudeSessionClient, CodexSessionClient, ModelTierCatalog, SessionEffort,
    VerifySessionOk, model::AgentModel, task_content,
};
use crate::{
    AppRecordStore, NoteMarkdownSource, PendingWorkItem,
    pending_work::{
        find_pending_work::{FindPendingWorkError, find_open_item},
        project_registry::ProjectRegistry,
        session::model_selection::resolve_model,
        show_pending_work_item::ShowPendingWorkError,
    },
};

/// Requests optional pending-work launchability context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifySession {
    pub id: Option<String>,
    pub agent: Agent,
    pub model_override: AgentModel,
}

#[derive(Debug, Error)]
pub enum VerifySessionError {
    #[error(transparent)]
    Find(#[from] FindPendingWorkError),
    #[error(transparent)]
    Show(#[from] ShowPendingWorkError),
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
    store: &impl AppRecordStore<PendingWorkItem>,
    projects: &ProjectRegistry,
    markdown_source: &impl NoteMarkdownSource,
    model_tiers: &impl ModelTierCatalog,
    claude: &impl ClaudeSessionClient,
    codex: &impl CodexSessionClient,
) -> Result<VerifySessionOk, VerifySessionError> {
    let probe = match query.agent {
        Agent::Claude => claude.probe(),
        Agent::Codex => codex.probe(),
    };
    let planned = prepare_verification(query, store, projects, markdown_source, model_tiers)?;
    let command_argv = match planned.launch.as_ref() {
        Some(launch) => match launch.agent {
            Agent::Claude => claude.preview(launch),
            Agent::Codex => codex.preview(launch),
        },
        None => vec![probe.binary.clone()],
    };

    Ok(VerifySessionOk {
        task_id: planned.task_id,
        probe,
        launchable: planned.launchable,
        issues: planned.issues,
        command_argv,
    })
}

struct PreparedVerification {
    task_id: Option<String>,
    launchable: bool,
    issues: Vec<String>,
    launch: Option<AgentLaunch>,
}

fn prepare_verification(
    query: VerifySession,
    store: &impl AppRecordStore<PendingWorkItem>,
    projects: &ProjectRegistry,
    markdown_source: &impl NoteMarkdownSource,
    model_tiers: &impl ModelTierCatalog,
) -> Result<PreparedVerification, VerifySessionError> {
    let Some(id) = query.id else {
        return Ok(PreparedVerification {
            task_id: None,
            launchable: true,
            issues: Vec::new(),
            launch: None,
        });
    };

    let item = find_open_item(store, projects, &id)?;
    let (model, model_issue) = match query.model_override.into_inner() {
        Some(model) => (Some(model), None),
        None => match resolve_model(model_tiers, query.agent, &item.id, item.effort.as_deref()) {
            Ok(model) => (model, None),
            Err(issue) => (None, Some(issue)),
        },
    };
    let task_content = if item.launchable {
        task_content::load(&item.id, store, projects, markdown_source)?
    } else {
        item.prompt.clone()
    };
    let launch = AgentLaunch::new(
        &item,
        &task_content,
        super::LaunchDirectives::default(),
        query.agent,
        model,
        SessionEffort::default(),
    );
    let mut issues = item.issues;
    let mut launchable = item.launchable;
    if let Some(issue) = model_issue {
        launchable = false;
        issues.push(issue.to_string());
    }
    Ok(PreparedVerification {
        task_id: Some(item.id),
        launchable,
        issues,
        launch: Some(launch),
    })
}

#[cfg(test)]
mod tests {
    use std::{convert::Infallible, error::Error, fmt, path::Path};

    use pwf_domain::pending_work::{
        EffortTier, ProjectName, Timestamp, WorkItemId, WorkItemStatus,
    };

    use super::{AgentModel, ProjectRegistry, VerifySession};
    use crate::{
        IndexPlacement, Materialization, NoteMarkdownSource, PendingWorkItem, RecordId,
        pending_work::session::{Agent, ModelTierCatalog, ModelTierLookup},
        testing::InMemoryStore,
    };

    const REPOSITORY: &str = "/repo/pwf";
    const TASK_ID: &str = "PWF-0139";

    #[derive(Clone)]
    struct UnusedNoteMarkdownSource;

    impl NoteMarkdownSource for UnusedNoteMarkdownSource {
        type Error = Infallible;

        fn read_note_markdown(&self, _path: &Path) -> Result<String, Self::Error> {
            panic!("non-launchable verification must not read note Markdown")
        }
    }

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
            effort: Some("highest".to_string()),
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

        let outcome = super::prepare_verification(
            VerifySession {
                id: Some(TASK_ID.to_string()),
                agent: Agent::Claude,
                model_override: AgentModel::default(),
            },
            &store,
            &projects,
            &UnusedNoteMarkdownSource,
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
