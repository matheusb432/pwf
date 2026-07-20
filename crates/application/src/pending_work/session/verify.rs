//! Verifies agent availability and pending-work launchability without dispatching.

use thiserror::Error;

use super::{
    Agent, ModelTierCatalog, SessionRuntime, VerifySessionOutcome, launch::build_agent_launch,
    model_selection::resolve_model_for_verify,
};
use crate::{
    AppDbStore, PendingWorkItem,
    pending_work::{
        find::{FindPendingWorkError, find_open_item},
        project_registry::ProjectRegistry,
    },
};

/// Requests an agent probe with optional item launchability context.
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

/// Probes an agent and optionally verifies one open item.
///
/// # Errors
///
/// Returns [`VerifySessionError`] when the requested item cannot be read. Model-tier failures
/// remain soft issues in [`VerifySessionOutcome`].
#[cqrsy::query]
pub fn execute(
    query: VerifySession,
    store: &impl AppDbStore<PendingWorkItem>,
    projects: &ProjectRegistry,
    model_tiers: &impl ModelTierCatalog,
    runtime: &impl SessionRuntime,
) -> Result<VerifySessionOutcome, VerifySessionError> {
    let probe = runtime.probe_agent(query.agent);
    let Some(id) = query.id else {
        let command_preview = probe.binary.clone();
        return Ok(VerifySessionOutcome {
            task_id: None,
            probe,
            launchable: true,
            issues: Vec::new(),
            command_preview,
        });
    };

    let item = find_open_item(store, projects, &id)?;
    let model = resolve_model_for_verify(
        model_tiers,
        query.agent,
        &item.id,
        item.effort.as_deref(),
        query.model_override.as_deref(),
    );
    let (model, model_issue) = match model {
        None => (None, None),
        Some(Ok(model)) => (Some(model), None),
        Some(Err(issue)) => (None, Some(issue)),
    };
    let launch = build_agent_launch(
        &item,
        super::LaunchDirectives::default(),
        query.agent,
        model,
    );
    let command_preview = runtime.command_preview(&launch);
    let mut issues = item.issues;
    let mut launchable = item.launchable;
    if let Some(issue) = model_issue {
        launchable = false;
        issues.push(issue);
    }
    Ok(VerifySessionOutcome {
        task_id: Some(item.id),
        probe,
        launchable,
        issues,
        command_preview,
    })
}

#[cfg(test)]
mod tests {
    use std::{
        error::Error,
        fmt,
        sync::{Arc, Mutex, MutexGuard},
    };

    use pwf_domain::pending_work::{
        EffortTier, ProjectName, Timestamp, WorkItemId, WorkItemStatus,
    };

    use super::{ProjectRegistry, VerifySession, execute};
    use crate::{
        IndexPlacement, Materialization, PendingWorkItem, RecordId,
        pending_work::session::{
            Agent, AgentLaunch, AgentProbe, DispatchTarget, ModelTier, ModelTierCatalog,
            ModelTierLookup, SessionRuntime, TabOpenError,
        },
        testing::InMemoryStore,
    };

    const CATALOG_PATH: &str = "/config/model-tiers.toml";
    const REPOSITORY: &str = "/repo/pwf";
    const TASK_ID: &str = "PWF-0139";

    #[derive(Debug, Default)]
    struct RuntimeState {
        available: bool,
        previews: Vec<AgentLaunch>,
    }

    #[derive(Debug, Clone, Default)]
    struct Runtime {
        state: Arc<Mutex<RuntimeState>>,
    }

    impl Runtime {
        fn lock(&self) -> MutexGuard<'_, RuntimeState> {
            self.state.lock().expect("runtime state lock poisoned")
        }

        fn available() -> Self {
            let runtime = Self::default();
            runtime.lock().available = true;
            runtime
        }
    }

    impl SessionRuntime for Runtime {
        fn probe_agent(&self, agent: Agent) -> AgentProbe {
            let binary = match agent {
                Agent::Claude => "claude",
                Agent::Codex => "codex",
            };
            let available = self.lock().available;
            AgentProbe {
                binary: binary.to_string(),
                available,
                path: available.then(|| format!("/bin/{binary}")),
                version: available.then(|| "1.0.0".to_string()),
            }
        }

        fn repository_is_directory(&self, _path: &str) -> bool {
            true
        }

        fn multiplexer_available(&self) -> bool {
            true
        }

        fn command_preview(&self, launch: &AgentLaunch) -> String {
            self.lock().previews.push(launch.clone());
            format!(
                "{}:{}",
                match launch.agent {
                    Agent::Claude => "claude",
                    Agent::Codex => "codex",
                },
                launch.model.as_deref().unwrap_or("default")
            )
        }

        fn run_inline(&self, _launch: &AgentLaunch) -> Result<(), String> {
            unreachable!("verify does not dispatch")
        }

        fn open_tab(
            &self,
            _target: &DispatchTarget,
            _launch: &AgentLaunch,
        ) -> Result<(), TabOpenError> {
            unreachable!("verify does not dispatch")
        }

        fn ensure_session(&self, _session: &str) -> Result<(), String> {
            unreachable!("verify does not dispatch")
        }
    }

    #[derive(Debug, Clone)]
    struct CatalogError(&'static str);

    impl fmt::Display for CatalogError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str(self.0)
        }
    }

    impl Error for CatalogError {}

    #[derive(Debug, Clone)]
    struct Catalog {
        result: Result<ModelTierLookup, CatalogError>,
    }

    impl ModelTierCatalog for Catalog {
        type Error = CatalogError;

        fn tier(&self, _effort: EffortTier) -> Result<ModelTierLookup, Self::Error> {
            self.result.clone()
        }
    }

    fn record(body: &str, effort: Option<&str>) -> PendingWorkItem {
        PendingWorkItem {
            id: RecordId::Item(WorkItemId::try_new(TASK_ID).unwrap()),
            title: "application verify".to_string(),
            status: WorkItemStatus::Active,
            created: Some(Timestamp::new("2026-07-15")),
            completed: None,
            commits: None,
            tags: None,
            effort: effort.map(str::to_string),
            prereq: None,
            section: None,
            body: body.to_string(),
            source: String::new(),
            locator: format!("/notes/{TASK_ID}.md"),
            placement: Some(IndexPlacement {
                index_path: "/notes/pwf.md".to_string(),
                line: 1,
            }),
            materialization: Materialization::NoteFile,
        }
    }

    fn store(record: PendingWorkItem) -> InMemoryStore {
        InMemoryStore::default().with_project("pwf", vec![record])
    }

    fn projects() -> ProjectRegistry {
        ProjectRegistry::new([(
            ProjectName::try_new("pwf").unwrap(),
            Some(REPOSITORY.to_string()),
            Some("PWF".to_string()),
        )])
    }

    fn catalog(model: Option<&str>) -> Catalog {
        Catalog {
            result: Ok(ModelTierLookup {
                catalog: CATALOG_PATH.to_string(),
                tier: Some(ModelTier {
                    claude_model: model.map(str::to_string),
                }),
            }),
        }
    }

    fn query(id: Option<&str>, agent: Agent) -> VerifySession {
        VerifySession {
            id: id.map(str::to_string),
            agent,
            model_override: None,
        }
    }

    #[test]
    fn no_item_returns_the_selected_binary_as_the_command_preview() {
        let runtime = Runtime::available();

        let outcome = execute(
            query(None, Agent::Claude),
            &InMemoryStore::default(),
            &projects(),
            &catalog(Some("sonnet")),
            &runtime,
        )
        .unwrap();

        assert_eq!(outcome.task_id, None);
        assert!(outcome.launchable);
        assert!(outcome.issues.is_empty());
        assert_eq!(outcome.command_preview, "claude");
        assert!(runtime.lock().previews.is_empty());
    }

    #[test]
    fn available_agent_is_reported_without_changing_item_launchability() {
        let runtime = Runtime::available();

        let outcome = execute(
            query(Some(TASK_ID), Agent::Claude),
            &store(record("implement verify", None)),
            &projects(),
            &catalog(Some("sonnet")),
            &runtime,
        )
        .unwrap();

        assert!(outcome.probe.available);
        assert!(outcome.launchable);
    }

    #[test]
    fn missing_agent_is_reported_in_the_probe_without_rewriting_launchability() {
        let runtime = Runtime::default();

        let outcome = execute(
            query(Some(TASK_ID), Agent::Claude),
            &store(record("implement verify", None)),
            &projects(),
            &catalog(Some("sonnet")),
            &runtime,
        )
        .unwrap();

        assert!(!outcome.probe.available);
        assert!(outcome.launchable);
        assert!(outcome.issues.is_empty());
    }

    #[test]
    fn launchable_item_retains_its_semantic_command_preview() {
        let runtime = Runtime::available();

        let outcome = execute(
            query(Some(TASK_ID), Agent::Claude),
            &store(record("implement verify", None)),
            &projects(),
            &catalog(Some("sonnet")),
            &runtime,
        )
        .unwrap();

        assert!(outcome.launchable);
        assert_eq!(outcome.command_preview, "claude:default");
        assert_eq!(runtime.lock().previews[0].task_id, TASK_ID);
    }

    #[test]
    fn non_launchable_item_retains_all_enrichment_issues() {
        let runtime = Runtime::available();

        let outcome = execute(
            query(Some(TASK_ID), Agent::Claude),
            &store(record("TODO", None)),
            &projects(),
            &catalog(Some("sonnet")),
            &runtime,
        )
        .unwrap();

        assert!(!outcome.launchable);
        assert!(
            outcome
                .issues
                .iter()
                .any(|issue| issue.contains("placeholder"))
        );
        assert_eq!(outcome.command_preview, "claude:default");
    }

    #[test]
    fn claude_effort_resolves_the_catalog_model() {
        let runtime = Runtime::available();

        let outcome = execute(
            query(Some(TASK_ID), Agent::Claude),
            &store(record("implement verify", Some("3"))),
            &projects(),
            &catalog(Some("opus")),
            &runtime,
        )
        .unwrap();

        assert_eq!(outcome.command_preview, "claude:opus");
        assert_eq!(runtime.lock().previews[0].model.as_deref(), Some("opus"));
    }

    #[test]
    fn explicit_override_wins_before_effort_or_catalog_validation() {
        let runtime = Runtime::available();
        let broken = Catalog {
            result: Err(CatalogError("catalog unavailable")),
        };
        let mut request = query(Some(TASK_ID), Agent::Claude);
        request.model_override = Some("fable".to_string());

        let outcome = execute(
            request,
            &store(record("implement verify", Some("nine"))),
            &projects(),
            &broken,
            &runtime,
        )
        .unwrap();

        assert!(outcome.launchable);
        assert!(outcome.issues.is_empty());
        assert_eq!(outcome.command_preview, "claude:fable");
    }

    #[test]
    fn empty_model_is_the_no_override_sentinel() {
        let runtime = Runtime::available();

        let outcome = execute(
            query(Some(TASK_ID), Agent::Claude),
            &store(record("implement verify", Some("2"))),
            &projects(),
            &catalog(Some("")),
            &runtime,
        )
        .unwrap();

        assert!(outcome.launchable);
        assert_eq!(outcome.command_preview, "claude:default");
        assert_eq!(runtime.lock().previews[0].model, None);
    }

    #[test]
    fn broken_model_tier_is_appended_as_a_soft_issue() {
        let runtime = Runtime::available();
        let broken = Catalog {
            result: Err(CatalogError("catalog unavailable")),
        };

        let outcome = execute(
            query(Some(TASK_ID), Agent::Claude),
            &store(record("implement verify", Some("4"))),
            &projects(),
            &broken,
            &runtime,
        )
        .unwrap();

        assert!(!outcome.launchable);
        assert_eq!(outcome.issues, ["catalog unavailable"]);
        assert_eq!(outcome.command_preview, "claude:default");
    }

    #[test]
    fn codex_ignores_effort_and_broken_tier_configuration() {
        let runtime = Runtime::available();
        let broken = Catalog {
            result: Err(CatalogError("catalog unavailable")),
        };

        let outcome = execute(
            query(Some(TASK_ID), Agent::Codex),
            &store(record("implement verify", Some("nine"))),
            &projects(),
            &broken,
            &runtime,
        )
        .unwrap();

        assert!(outcome.launchable);
        assert!(outcome.issues.is_empty());
        assert_eq!(outcome.command_preview, "codex:default");
        assert_eq!(runtime.lock().previews[0].model, None);
    }
}
