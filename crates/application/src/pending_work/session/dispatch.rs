//! Dispatches a pending-work item through application-owned session policy.

use std::error::Error;

use pwf_domain::pending_work::ProjectRegistry;
use thiserror::Error;

use super::{
    Agent, ConfirmationPolicy, DispatchConfirmation, DispatchMode, DispatchSessionOutcome,
    LaunchDirectives, ModelTierCatalog, SessionInteraction, SessionRuntime, TabOpenError,
    launch::{build_agent_launch, dispatch_target},
    model_selection::resolve_model,
};
use crate::{
    AppDbStore, PendingWorkItem,
    pending_work::find::{FindPendingWorkError, find_open_item},
};

/// Requests one inline or multiplexer session dispatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchSession {
    pub id: String,
    pub mode: DispatchMode,
    pub directives: LaunchDirectives,
    pub agent: Agent,
    pub model_override: Option<String>,
    pub confirmation: ConfirmationPolicy,
}

/// Reports failures that prevent a dispatch outcome from being produced.
#[derive(Debug, Error)]
pub enum DispatchSessionError {
    #[error(transparent)]
    Find(#[from] FindPendingWorkError),
    #[error("Pending-work item '{id}' is not launchable: {}", issues.join("; "))]
    NotLaunchable { id: String, issues: Vec<String> },
    #[error("Repo directory for project '{project}' does not exist: {path}")]
    RepositoryMissing { project: String, path: String },
    #[error("zellij not found on PATH; cannot dispatch a pwf session (Linux-only feature).")]
    MultiplexerNotFound,
    /// Preserves the model-catalog adapter's source chain.
    #[error("{0}")]
    ModelTier(#[source] Box<dyn Error + Send + Sync>),
    #[error("Failed to dispatch into zellij session '{session}': {message}")]
    SessionEnsureFailed { session: String, message: String },
    #[error("Failed to run agent inline: {message}")]
    InlineFailed { message: String },
}

/// Dispatches one open item through the selected runtime.
///
/// # Errors
///
/// Returns [`DispatchSessionError`] for lookup, validation, model selection, inline execution, or
/// session recovery failures. Other tab-open failures become [`DispatchSessionOutcome::Failed`].
#[cqrsy::command]
pub fn execute(
    command: &DispatchSession,
    store: &impl AppDbStore<PendingWorkItem>,
    projects: &ProjectRegistry,
    model_tiers: &impl ModelTierCatalog,
    runtime: &impl SessionRuntime,
    interaction: &impl SessionInteraction,
) -> Result<DispatchSessionOutcome, DispatchSessionError> {
    let probe = runtime.probe_agent(command.agent);
    if !probe.available {
        interaction.warn_agent_missing(&probe.binary);
    }

    let item = find_open_item(store, projects, &command.id)?;
    if !item.launchable {
        return Err(DispatchSessionError::NotLaunchable {
            id: item.id,
            issues: item.issues,
        });
    }

    let model = resolve_model(
        model_tiers,
        command.agent,
        &item.id,
        item.effort.as_deref(),
        command.model_override.as_deref(),
    )
    .map_err(|error| DispatchSessionError::ModelTier(Box::new(error)))?;

    let repository = item.repo.clone().unwrap_or_default();
    if !runtime.repository_is_directory(&repository) {
        return Err(DispatchSessionError::RepositoryMissing {
            project: item.project.clone(),
            path: repository,
        });
    }

    if command.mode == DispatchMode::Multiplexer && !runtime.multiplexer_available() {
        return Err(DispatchSessionError::MultiplexerNotFound);
    }

    let target = dispatch_target(&item.id);
    let launch = build_agent_launch(&item, command.directives, command.agent, model);
    if command.confirmation == ConfirmationPolicy::Ask {
        let confirmation = DispatchConfirmation {
            task_id: item.id.clone(),
            title: item.session.clone(),
            created: item.created.clone(),
            mode: command.mode,
            agent: command.agent,
            directives: command.directives,
            target: target.clone(),
        };
        if !interaction.confirm(&confirmation) {
            return Ok(DispatchSessionOutcome::Aborted { task_id: item.id });
        }
    }

    match command.mode {
        DispatchMode::Inline => {
            interaction.inline_starting(&item.id, &repository);
            runtime
                .run_inline(&launch)
                .map_err(|message| DispatchSessionError::InlineFailed { message })?;
            Ok(DispatchSessionOutcome::Inline { task_id: item.id })
        }
        DispatchMode::Multiplexer => dispatch_multiplexer(runtime, target, &launch),
    }
}

fn dispatch_multiplexer(
    runtime: &impl SessionRuntime,
    target: super::DispatchTarget,
    launch: &super::AgentLaunch,
) -> Result<DispatchSessionOutcome, DispatchSessionError> {
    match runtime.open_tab(&target, launch) {
        Ok(()) => Ok(direct_outcome(target, launch)),
        Err(TabOpenError::Other(message)) => Ok(DispatchSessionOutcome::Failed { target, message }),
        Err(TabOpenError::SessionNotFound) => {
            runtime.ensure_session(&target.session).map_err(|message| {
                DispatchSessionError::SessionEnsureFailed {
                    session: target.session.clone(),
                    message,
                }
            })?;
            match runtime.open_tab(&target, launch) {
                Ok(()) => Ok(recovered_outcome(target, launch)),
                Err(error) => Ok(DispatchSessionOutcome::Failed {
                    target,
                    message: tab_error_message(error),
                }),
            }
        }
    }
}

fn direct_outcome(
    target: super::DispatchTarget,
    launch: &super::AgentLaunch,
) -> DispatchSessionOutcome {
    DispatchSessionOutcome::Direct {
        target,
        agent: launch.agent,
        repository: launch.repository.clone(),
    }
}

fn recovered_outcome(
    target: super::DispatchTarget,
    launch: &super::AgentLaunch,
) -> DispatchSessionOutcome {
    DispatchSessionOutcome::Recovered {
        target,
        agent: launch.agent,
        repository: launch.repository.clone(),
    }
}

fn tab_error_message(error: TabOpenError) -> String {
    match error {
        TabOpenError::SessionNotFound => "session not found".to_string(),
        TabOpenError::Other(message) => message,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{BTreeSet, VecDeque},
        error::Error,
        fmt,
        sync::{Arc, Mutex, MutexGuard},
    };

    use pwf_domain::pending_work::{
        EffortTier, ProjectName, ProjectRegistry, Timestamp, WorkItemId, WorkItemStatus,
    };

    use super::{DispatchSession, DispatchSessionError, execute};
    use crate::{
        IndexPlacement, Materialization, PendingWorkItem, RecordId,
        pending_work::{
            find::FindPendingWorkError,
            session::{
                Agent, AgentLaunch, AgentProbe, ConfirmationPolicy, DispatchConfirmation,
                DispatchMode, DispatchSessionOutcome, DispatchTarget, LaunchDirectives, ModelTier,
                ModelTierCatalog, ModelTierLookup, SessionInteraction, SessionRuntime,
                TabOpenError,
            },
        },
        testing::InMemoryStore,
    };

    const CATALOG_PATH: &str = "/config/model-tiers.toml";
    const REPOSITORY: &str = "/repo/pwf";
    const TASK_ID: &str = "PWF-0139";

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Event {
        Warning(String),
        Confirmation,
        InlineStarting,
        InlineRun,
    }

    #[derive(Debug)]
    struct RuntimeState {
        available_agent: bool,
        repositories: BTreeSet<String>,
        multiplexer_available: bool,
        tab_results: VecDeque<Result<(), TabOpenError>>,
        created_sessions: Vec<String>,
        ensure_result: Result<(), String>,
        inline_result: Result<(), String>,
        inline_launches: Vec<AgentLaunch>,
        tab_launches: Vec<(DispatchTarget, AgentLaunch)>,
        events: Vec<Event>,
    }

    impl Default for RuntimeState {
        fn default() -> Self {
            Self {
                available_agent: true,
                repositories: BTreeSet::new(),
                multiplexer_available: true,
                tab_results: VecDeque::new(),
                created_sessions: Vec::new(),
                ensure_result: Ok(()),
                inline_result: Ok(()),
                inline_launches: Vec::new(),
                tab_launches: Vec::new(),
                events: Vec::new(),
            }
        }
    }

    #[derive(Debug, Clone, Default)]
    struct Runtime {
        state: Arc<Mutex<RuntimeState>>,
    }

    impl Runtime {
        fn lock(&self) -> MutexGuard<'_, RuntimeState> {
            self.state.lock().expect("runtime state lock poisoned")
        }

        fn with_repository(self) -> Self {
            self.lock().repositories.insert(REPOSITORY.to_string());
            self
        }

        fn with_tab_results(
            self,
            results: impl IntoIterator<Item = Result<(), TabOpenError>>,
        ) -> Self {
            self.lock().tab_results = results.into_iter().collect();
            self
        }
    }

    impl SessionRuntime for Runtime {
        fn probe_agent(&self, agent: Agent) -> AgentProbe {
            let binary = match agent {
                Agent::Claude => "claude",
                Agent::Codex => "codex",
            };
            let available = self.lock().available_agent;
            AgentProbe {
                binary: binary.to_string(),
                available,
                path: available.then(|| format!("/bin/{binary}")),
                version: available.then(|| "1.0.0".to_string()),
            }
        }

        fn repository_is_directory(&self, path: &str) -> bool {
            self.lock().repositories.contains(path)
        }

        fn multiplexer_available(&self) -> bool {
            self.lock().multiplexer_available
        }

        fn command_preview(&self, launch: &AgentLaunch) -> String {
            format!(
                "{}:{}",
                launch.task_id,
                launch.model.as_deref().unwrap_or("default")
            )
        }

        fn run_inline(&self, launch: &AgentLaunch) -> Result<(), String> {
            let mut state = self.lock();
            state.inline_launches.push(launch.clone());
            state.events.push(Event::InlineRun);
            state.inline_result.clone()
        }

        fn open_tab(
            &self,
            target: &DispatchTarget,
            launch: &AgentLaunch,
        ) -> Result<(), TabOpenError> {
            let mut state = self.lock();
            state.tab_launches.push((target.clone(), launch.clone()));
            state
                .tab_results
                .pop_front()
                .unwrap_or_else(|| panic!("stage a tab result before opening {}", target.tab))
        }

        fn ensure_session(&self, session: &str) -> Result<(), String> {
            let mut state = self.lock();
            state.created_sessions.push(session.to_string());
            state.ensure_result.clone()
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

    #[derive(Debug)]
    struct InteractionState {
        answer: bool,
        warnings: Vec<String>,
        confirmations: Vec<DispatchConfirmation>,
    }

    #[derive(Debug, Clone)]
    struct Interaction {
        state: Arc<Mutex<InteractionState>>,
        runtime: Runtime,
    }

    impl Interaction {
        fn new(runtime: &Runtime, answer: bool) -> Self {
            Self {
                state: Arc::new(Mutex::new(InteractionState {
                    answer,
                    warnings: Vec::new(),
                    confirmations: Vec::new(),
                })),
                runtime: runtime.clone(),
            }
        }

        fn lock(&self) -> MutexGuard<'_, InteractionState> {
            self.state.lock().expect("interaction state lock poisoned")
        }
    }

    impl SessionInteraction for Interaction {
        fn warn_agent_missing(&self, binary: &str) {
            self.lock().warnings.push(binary.to_string());
            self.runtime
                .lock()
                .events
                .push(Event::Warning(binary.to_string()));
        }

        fn confirm(&self, context: &DispatchConfirmation) -> bool {
            let mut state = self.lock();
            state.confirmations.push(context.clone());
            let answer = state.answer;
            drop(state);
            self.runtime.lock().events.push(Event::Confirmation);
            answer
        }

        fn inline_starting(&self, _task_id: &str, _repository: &str) {
            self.runtime.lock().events.push(Event::InlineStarting);
        }
    }

    fn record(body: &str, effort: Option<&str>) -> PendingWorkItem {
        PendingWorkItem {
            id: RecordId::Item(WorkItemId::try_new(TASK_ID).unwrap()),
            title: "application dispatch".to_string(),
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

    fn missing_tier_catalog() -> Catalog {
        Catalog {
            result: Ok(ModelTierLookup {
                catalog: CATALOG_PATH.to_string(),
                tier: None,
            }),
        }
    }

    fn command(mode: DispatchMode) -> DispatchSession {
        DispatchSession {
            id: TASK_ID.to_string(),
            mode,
            directives: LaunchDirectives::default(),
            agent: Agent::Claude,
            model_override: None,
            confirmation: ConfirmationPolicy::Skip,
        }
    }

    fn dispatch(
        command: &DispatchSession,
        store: &InMemoryStore,
        catalog: &Catalog,
        runtime: &Runtime,
        interaction: &Interaction,
    ) -> Result<DispatchSessionOutcome, DispatchSessionError> {
        execute(command, store, &projects(), catalog, runtime, interaction)
    }

    #[test]
    fn unknown_item_is_a_lookup_error() {
        let store = InMemoryStore::default().with_project("pwf", Vec::new());
        let runtime = Runtime::default();
        let interaction = Interaction::new(&runtime, true);

        let error = dispatch(
            &command(DispatchMode::Inline),
            &store,
            &catalog(Some("sonnet")),
            &runtime,
            &interaction,
        )
        .unwrap_err();

        assert!(matches!(
            error,
            DispatchSessionError::Find(FindPendingWorkError::ItemNotFound { ref id })
                if id == TASK_ID
        ));
    }

    #[test]
    fn non_launchable_item_is_rejected_before_runtime_dispatch() {
        let store = store(record("TODO", None));
        let runtime = Runtime::default().with_repository();
        let interaction = Interaction::new(&runtime, true);

        let error = dispatch(
            &command(DispatchMode::Inline),
            &store,
            &catalog(Some("sonnet")),
            &runtime,
            &interaction,
        )
        .unwrap_err();

        assert!(matches!(
            error,
            DispatchSessionError::NotLaunchable { ref id, ref issues }
                if id == TASK_ID && issues.iter().any(|issue| issue.contains("placeholder"))
        ));
        assert!(runtime.lock().inline_launches.is_empty());
    }

    #[test]
    fn missing_repository_is_rejected_before_execution() {
        let store = store(record("implement dispatch", None));
        let runtime = Runtime::default();
        let interaction = Interaction::new(&runtime, true);

        let error = dispatch(
            &command(DispatchMode::Inline),
            &store,
            &catalog(Some("sonnet")),
            &runtime,
            &interaction,
        )
        .unwrap_err();

        assert!(matches!(
            error,
            DispatchSessionError::RepositoryMissing { ref project, ref path }
                if project == "pwf" && path == REPOSITORY
        ));
    }

    #[test]
    fn model_tier_failure_precedes_missing_repository() {
        let store = store(record("implement dispatch", Some("3")));
        let runtime = Runtime::default();
        let interaction = Interaction::new(&runtime, true);

        let error = dispatch(
            &command(DispatchMode::Inline),
            &store,
            &missing_tier_catalog(),
            &runtime,
            &interaction,
        )
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            format!("tier 3 has no [tiers.3] entry in {CATALOG_PATH}")
        );
        assert!(error.source().is_some());
    }

    #[test]
    fn missing_multiplexer_is_rejected_only_for_multiplexer_mode() {
        let store = store(record("implement dispatch", None));
        let runtime = Runtime::default().with_repository();
        runtime.lock().multiplexer_available = false;
        let interaction = Interaction::new(&runtime, true);

        let error = dispatch(
            &command(DispatchMode::Multiplexer),
            &store,
            &catalog(Some("sonnet")),
            &runtime,
            &interaction,
        )
        .unwrap_err();

        assert!(matches!(error, DispatchSessionError::MultiplexerNotFound));
    }

    #[test]
    fn declined_confirmation_aborts_without_dispatching() {
        let store = store(record("implement dispatch", None));
        let runtime = Runtime::default().with_repository();
        let interaction = Interaction::new(&runtime, false);
        let mut request = command(DispatchMode::Multiplexer);
        request.confirmation = ConfirmationPolicy::Ask;

        let outcome = dispatch(
            &request,
            &store,
            &catalog(Some("sonnet")),
            &runtime,
            &interaction,
        )
        .unwrap();

        assert_eq!(
            outcome,
            DispatchSessionOutcome::Aborted {
                task_id: TASK_ID.to_string()
            }
        );
        assert!(runtime.lock().tab_launches.is_empty());
        let confirmations = &interaction.lock().confirmations;
        assert_eq!(confirmations[0].task_id, TASK_ID);
        assert_eq!(confirmations[0].mode, DispatchMode::Multiplexer);
    }

    #[test]
    fn inline_success_reports_starting_before_running() {
        let store = store(record("implement dispatch", None));
        let runtime = Runtime::default().with_repository();
        let interaction = Interaction::new(&runtime, true);

        let outcome = dispatch(
            &command(DispatchMode::Inline),
            &store,
            &catalog(Some("sonnet")),
            &runtime,
            &interaction,
        )
        .unwrap();

        assert_eq!(
            outcome,
            DispatchSessionOutcome::Inline {
                task_id: TASK_ID.to_string()
            }
        );
        let state = runtime.lock();
        assert_eq!(state.events, [Event::InlineStarting, Event::InlineRun]);
        assert_eq!(state.inline_launches[0].repository, REPOSITORY);
    }

    #[test]
    fn inline_failure_is_an_operation_error() {
        let store = store(record("implement dispatch", None));
        let runtime = Runtime::default().with_repository();
        runtime.lock().inline_result = Err("exec refused".to_string());
        let interaction = Interaction::new(&runtime, true);

        let error = dispatch(
            &command(DispatchMode::Inline),
            &store,
            &catalog(Some("sonnet")),
            &runtime,
            &interaction,
        )
        .unwrap_err();

        assert!(matches!(
            error,
            DispatchSessionError::InlineFailed { ref message } if message == "exec refused"
        ));
    }

    #[test]
    fn direct_tab_success_is_a_direct_outcome() {
        let store = store(record("implement dispatch", None));
        let runtime = Runtime::default()
            .with_repository()
            .with_tab_results([Ok(())]);
        let interaction = Interaction::new(&runtime, true);

        let outcome = dispatch(
            &command(DispatchMode::Multiplexer),
            &store,
            &catalog(Some("sonnet")),
            &runtime,
            &interaction,
        )
        .unwrap();

        assert!(matches!(
            outcome,
            DispatchSessionOutcome::Direct { ref target, agent: Agent::Claude, ref repository }
                if target.session == "pwf" && target.tab == TASK_ID && repository == REPOSITORY
        ));
    }

    #[test]
    fn missing_session_is_ensured_and_retried_once() {
        let store = store(record("implement dispatch", None));
        let runtime = Runtime::default()
            .with_repository()
            .with_tab_results([Err(TabOpenError::SessionNotFound), Ok(())]);
        let interaction = Interaction::new(&runtime, true);

        let outcome = dispatch(
            &command(DispatchMode::Multiplexer),
            &store,
            &catalog(Some("sonnet")),
            &runtime,
            &interaction,
        )
        .unwrap();

        assert!(matches!(outcome, DispatchSessionOutcome::Recovered { .. }));
        let state = runtime.lock();
        assert_eq!(state.created_sessions, ["pwf"]);
        assert!(state.tab_results.is_empty());
    }

    #[test]
    fn ensure_session_failure_is_an_operation_error() {
        let store = store(record("implement dispatch", None));
        let runtime = Runtime::default()
            .with_repository()
            .with_tab_results([Err(TabOpenError::SessionNotFound)]);
        runtime.lock().ensure_result = Err("create refused".to_string());
        let interaction = Interaction::new(&runtime, true);

        let error = dispatch(
            &command(DispatchMode::Multiplexer),
            &store,
            &catalog(Some("sonnet")),
            &runtime,
            &interaction,
        )
        .unwrap_err();

        assert!(matches!(
            error,
            DispatchSessionError::SessionEnsureFailed { ref session, ref message }
                if session == "pwf" && message == "create refused"
        ));
    }

    #[test]
    fn retry_failure_is_a_failed_outcome_without_a_third_attempt() {
        let store = store(record("implement dispatch", None));
        let runtime = Runtime::default().with_repository().with_tab_results([
            Err(TabOpenError::SessionNotFound),
            Err(TabOpenError::Other("retry refused".to_string())),
            Ok(()),
        ]);
        let interaction = Interaction::new(&runtime, true);

        let outcome = dispatch(
            &command(DispatchMode::Multiplexer),
            &store,
            &catalog(Some("sonnet")),
            &runtime,
            &interaction,
        )
        .unwrap();

        assert!(matches!(
            outcome,
            DispatchSessionOutcome::Failed { ref target, ref message }
                if target.session == "pwf" && message == "retry refused"
        ));
        assert_eq!(runtime.lock().tab_results, VecDeque::from([Ok(())]));
    }

    #[test]
    fn initial_provider_failure_is_a_failed_outcome() {
        let store = store(record("implement dispatch", None));
        let runtime = Runtime::default()
            .with_repository()
            .with_tab_results([Err(TabOpenError::Other("tab refused".to_string()))]);
        let interaction = Interaction::new(&runtime, true);

        let outcome = dispatch(
            &command(DispatchMode::Multiplexer),
            &store,
            &catalog(Some("sonnet")),
            &runtime,
            &interaction,
        )
        .unwrap();

        assert!(matches!(
            outcome,
            DispatchSessionOutcome::Failed { ref message, .. } if message == "tab refused"
        ));
    }

    #[test]
    fn missing_agent_warning_precedes_confirmation_and_inline_execution() {
        let store = store(record("implement dispatch", None));
        let runtime = Runtime::default().with_repository();
        runtime.lock().available_agent = false;
        let interaction = Interaction::new(&runtime, true);
        let mut request = command(DispatchMode::Inline);
        request.confirmation = ConfirmationPolicy::Ask;

        let outcome = dispatch(
            &request,
            &store,
            &catalog(Some("sonnet")),
            &runtime,
            &interaction,
        )
        .unwrap();

        assert!(matches!(outcome, DispatchSessionOutcome::Inline { .. }));
        assert_eq!(interaction.lock().warnings, ["claude"]);
        assert_eq!(
            runtime.lock().events,
            [
                Event::Warning("claude".to_string()),
                Event::Confirmation,
                Event::InlineStarting,
                Event::InlineRun,
            ]
        );
    }

    #[test]
    fn explicit_model_override_wins_before_effort_and_catalog_validation() {
        let store = store(record("implement dispatch", Some("nine")));
        let runtime = Runtime::default().with_repository();
        let interaction = Interaction::new(&runtime, true);
        let unavailable_catalog = Catalog {
            result: Err(CatalogError("catalog unavailable")),
        };
        let mut request = command(DispatchMode::Inline);
        request.model_override = Some("fable".to_string());

        dispatch(
            &request,
            &store,
            &unavailable_catalog,
            &runtime,
            &interaction,
        )
        .unwrap();

        assert_eq!(
            runtime.lock().inline_launches[0].model.as_deref(),
            Some("fable")
        );
    }

    #[test]
    fn model_tier_failure_is_a_hard_dispatch_error_with_catalog_provenance() {
        let store = store(record("implement dispatch", Some("3")));
        let runtime = Runtime::default().with_repository();
        let interaction = Interaction::new(&runtime, true);

        let error = dispatch(
            &command(DispatchMode::Inline),
            &store,
            &missing_tier_catalog(),
            &runtime,
            &interaction,
        )
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            format!("tier 3 has no [tiers.3] entry in {CATALOG_PATH}")
        );
        assert!(error.source().is_some());
    }
}
