//! Verifies pending-work launchability with the selected provider.

use std::error::Error;

use pwf_models::{pending_work::EffortTier, session::AgentModel};
use thiserror::Error;

use super::{Agent, AgentLaunch, ModelTierLookup, SessionEffort, VerifySessionOk, logic};
use crate::{
    pending_work::{
        find_pending_work::FindPendingWorkError, logic::finding::find_open_item_from_db,
        show_pending_work_item::ShowPendingWorkError,
    },
    ports::{
        agent::AgentClient, pending_work_record::PendingWorkStore, project_note::ProjectNoteStore,
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
pub async fn execute(
    query: VerifySession,
    store: &(impl PendingWorkStore + ProjectNoteStore),
    pool: &sqlx::SqlitePool,
    agent_client: &impl AgentClient,
) -> Result<VerifySessionOk, VerifySessionError> {
    let probe = agent_client.probe(query.agent);
    let planned =
        prepare_verification(query, store, pool, |effort| agent_client.model_tier(effort)).await?;
    let command_argv = match planned.launch.as_ref() {
        Some(launch) => agent_client.preview(launch),
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

async fn prepare_verification<E>(
    query: VerifySession,
    store: &(impl PendingWorkStore + ProjectNoteStore),
    pool: &sqlx::SqlitePool,
    model_tier: impl FnOnce(EffortTier) -> Result<ModelTierLookup, E>,
) -> Result<PreparedVerification, VerifySessionError>
where
    E: Error + Send + Sync + 'static,
{
    let Some(id) = query.id else {
        return Ok(PreparedVerification {
            task_id: None,
            launchable: true,
            issues: Vec::new(),
            launch: None,
        });
    };

    let item = find_open_item_from_db(store, pool, &id).await?;
    let (model, model_issue) = match query.model_override.into_inner() {
        Some(model) => (Some(model), None),
        None => {
            match logic::resolve_model(query.agent, &item.id, item.effort.as_deref(), model_tier) {
                Ok(model) => (model, None),
                Err(issue) => (None, Some(issue)),
            }
        }
    };
    let task_content = if item.launchable {
        logic::load_task_content(&item.id, store, pool).await?
    } else {
        item.prompt.clone()
    };
    let launch = logic::agent_launch(
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
    use std::{error::Error, fmt};

    use pwf_models::pending_work::{Timestamp, WorkItemId, WorkItemStatus};

    use super::{AgentModel, VerifySession};
    use crate::{
        pending_work::session::{Agent, ModelTierLookup},
        ports::pending_work_record::{
            IndexPlacement, Materialization, PendingWorkRecord, RecordId,
        },
        testing::{InMemoryStore, insert_project},
    };

    const REPOSITORY: &str = "/repo/pwf";
    const TASK_ID: &str = "PWF-0139";

    #[derive(Debug)]
    struct CatalogError;

    impl fmt::Display for CatalogError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("catalog unavailable")
        }
    }

    impl Error for CatalogError {}

    fn record() -> PendingWorkRecord {
        PendingWorkRecord {
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

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn item_and_model_issues_are_aggregated(pool: sqlx::SqlitePool) {
        insert_project(&pool, "PWF", "pwf", REPOSITORY, "/tasks/pwf", false).await;
        let store = InMemoryStore::default().with_project("pwf", vec![record()]);

        let outcome = super::prepare_verification(
            VerifySession {
                id: Some(TASK_ID.to_string()),
                agent: Agent::Claude,
                model_override: AgentModel::default(),
            },
            &store,
            &pool,
            |_| Err::<ModelTierLookup, _>(CatalogError),
        )
        .await
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
