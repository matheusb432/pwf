use std::convert::Infallible;

use pwf_application::{
    ports::agent::{AgentClient, PreparedAgentLaunch},
    task::session::{plan_session, plan_session::SessionPlanningClients},
};
use pwf_models::{
    project::HomeDirectory,
    session::{Agent, AgentModel, SessionEffort, SessionTaskIds},
    task::{EffortTier, TaskStatus},
};
use pwf_wire::task::{
    BlockedByIssue, BlockedByResolution, BlockedByStatus, StoredBlockedBy, TaskRecord,
    session::{
        AgentAvailability, AgentLaunch, AgentProbe, DryRunSession, PlanSession, PlanSessionIntent,
        PlannedSession, SessionWarning,
    },
};

use crate::support::{
    ExistingProjectDirectory, InMemoryStore, insert_project, stored_blocked_by, task_record,
};

#[derive(Clone, Copy)]
struct AgentStub;

impl AgentClient for AgentStub {
    type PreparationError = Infallible;

    fn probe(&self, agent: Agent) -> AgentProbe {
        AgentProbe {
            agent,
            availability: AgentAvailability::Available,
        }
    }

    fn preview(&self, _: &AgentLaunch) -> Vec<String> {
        vec!["claude".to_string()]
    }

    fn prepare(&self, _: &AgentLaunch) -> Result<PreparedAgentLaunch, Self::PreparationError> {
        Ok(PreparedAgentLaunch::Process {
            arguments: vec!["claude".to_string()],
        })
    }
}

fn command(model_override: Option<&str>) -> PlanSession {
    PlanSession {
        task_ids: SessionTaskIds::try_new(["FOO-0001".parse().unwrap()]).unwrap(),
        intent: PlanSessionIntent::DryRun,
        pushed_prompt: None,
        agent: Agent::Claude,
        model_override: AgentModel::from(model_override.map(str::to_string)),
        effort: SessionEffort::Max,
    }
}

async fn planned_model(
    command: &PlanSession,
    store: &InMemoryStore,
    pool: &sqlx::SqlitePool,
    clients: &SessionPlanningClients<AgentStub, ExistingProjectDirectory>,
) -> anyhow::Result<(AgentModel, SessionEffort)> {
    let dry_run = plan(command, store, pool, clients).await?;
    Ok((dry_run.plan.launch.model, dry_run.plan.launch.effort))
}

async fn plan(
    command: &PlanSession,
    store: &InMemoryStore,
    pool: &sqlx::SqlitePool,
    clients: &SessionPlanningClients<AgentStub, ExistingProjectDirectory>,
) -> anyhow::Result<DryRunSession> {
    let planned = plan_session::execute(
        command,
        store,
        pool,
        &HomeDirectory::new("/home/dev".into()),
        clients,
    )
    .await?;
    let PlannedSession::DryRun(dry_run) = planned else {
        anyhow::bail!("expected a dry-run plan");
    };
    Ok(dry_run)
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn planning_uses_only_the_explicit_model_override(pool: sqlx::SqlitePool) {
    insert_project(&pool, "FOO", "foo", "/work/foo", "/tasks/foo", false).await;
    let record = TaskRecord {
        effort: Some(EffortTier::Highest.to_string()),
        ..task_record("FOO-0001")
    };
    let store = InMemoryStore::default().with_project("foo", vec![record]);
    let clients = SessionPlanningClients::new(AgentStub, ExistingProjectDirectory);

    let (default_model, effort) = planned_model(&command(None), &store, &pool, &clients)
        .await
        .unwrap();
    let (explicit_model, _) =
        planned_model(&command(Some("manual-model")), &store, &pool, &clients)
            .await
            .unwrap();

    assert_eq!(default_model.as_deref(), None);
    assert_eq!(explicit_model.as_deref(), Some("manual-model"));
    assert_eq!(effort, SessionEffort::Max);
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn planning_uses_one_compound_identity_and_keeps_task_prompt_order(
    pool: sqlx::SqlitePool,
) -> anyhow::Result<()> {
    insert_project(&pool, "FOO", "foo", "/work/foo", "/tasks/foo", false).await;
    let first = TaskRecord {
        title: "first task".to_string(),
        source: "first task content".to_string(),
        ..task_record("FOO-0001")
    };
    let second = TaskRecord {
        title: "second task".to_string(),
        source: "second task content".to_string(),
        ..task_record("FOO-0002")
    };
    let store = InMemoryStore::default().with_project("foo", vec![first, second]);
    let clients = SessionPlanningClients::new(AgentStub, ExistingProjectDirectory);
    let mut command = command(None);
    command.task_ids =
        SessionTaskIds::try_new(["FOO-0002".parse().unwrap(), "FOO-0001".parse().unwrap()])
            .unwrap();

    let planned = plan_session::execute(
        &command,
        &store,
        &pool,
        &HomeDirectory::new("/home/dev".into()),
        &clients,
    )
    .await
    .unwrap();
    let PlannedSession::DryRun(dry_run) = planned else {
        anyhow::bail!("test command must produce a dry-run plan");
    };
    let launch = dry_run.plan.launch;
    let prompt = launch.prompt.as_ref();

    assert_eq!(launch.title.as_ref(), "foo1,foo2");
    assert_eq!(
        launch
            .task_ids
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        ["FOO-0002", "FOO-0001"]
    );
    assert!(
        prompt.find("second task content").unwrap() < prompt.find("first task content").unwrap()
    );
    assert_eq!(prompt.matches("<pwf_task>").count(), 2);
    Ok(())
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn direct_blocker_warnings_include_unresolved_missing_and_malformed_data(
    pool: sqlx::SqlitePool,
) {
    insert_project(&pool, "FOO", "foo", "/work/foo", "/tasks/foo", false).await;
    let clients = SessionPlanningClients::new(AgentStub, ExistingProjectDirectory);
    insert_project(
        &pool,
        "AUX",
        "paused-project",
        "/projects/paused",
        "/tasks/paused",
        true,
    )
    .await;
    let done = TaskRecord {
        status: TaskStatus::Done,
        ..task_record("AUX-0001")
    };
    let active = task_record("AUX-0002");
    let cancelled = TaskRecord {
        status: TaskStatus::Cancelled,
        ..task_record("AUX-0003")
    };
    let store =
        InMemoryStore::default().with_project("paused-project", vec![done, active, cancelled]);
    let target = TaskRecord {
        blocked_by: stored_blocked_by(&["AUX-0001", "AUX-0002", "AUX-0003", "AUX-9999"]),
        ..task_record("FOO-0001")
    };

    let store = store.with_project("foo", vec![target]);
    let warnings = plan(&command(None), &store, &pool, &clients)
        .await
        .unwrap()
        .warnings;

    assert_eq!(
        warnings,
        [
            SessionWarning::BlockedBy(BlockedByStatus {
                id: "AUX-0002".parse().unwrap(),
                title: Some("tray gui".to_string()),
                resolution: BlockedByResolution::Found(TaskStatus::Active),
            }),
            SessionWarning::BlockedBy(BlockedByStatus {
                id: "AUX-0003".parse().unwrap(),
                title: Some("tray gui".to_string()),
                resolution: BlockedByResolution::Found(TaskStatus::Cancelled),
            }),
            SessionWarning::BlockedBy(BlockedByStatus {
                id: "AUX-9999".parse().unwrap(),
                title: None,
                resolution: BlockedByResolution::Missing,
            }),
        ]
    );

    let malformed = TaskRecord {
        blocked_by: StoredBlockedBy::Malformed {
            raw: "\"[[AUX-0001]]\"".to_string(),
            reason: "expected a sequence".to_string(),
        },
        ..task_record("FOO-0001")
    };
    let path = malformed.locator.clone();
    let store = store.with_project("foo", vec![malformed]);
    assert_eq!(
        plan(&command(None), &store, &pool, &clients)
            .await
            .unwrap()
            .warnings,
        [SessionWarning::BlockedByMetadata(
            BlockedByIssue::Malformed {
                path,
                raw: "\"[[AUX-0001]]\"".to_string(),
                reason: "expected a sequence".to_string(),
            }
        )]
    );
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn planning_rejects_missing_ambiguous_and_closed_tasks(pool: sqlx::SqlitePool) {
    insert_project(&pool, "FOO", "foo", "/work/foo", "/tasks/foo", false).await;
    let clients = SessionPlanningClients::new(AgentStub, ExistingProjectDirectory);
    for (records, expected) in [
        (vec![], "Active task not found: FOO-0001"),
        (
            vec![task_record("FOO-0001"), task_record("FOO-0001")],
            "Task id is ambiguous: FOO-0001",
        ),
        (
            vec![TaskRecord {
                status: TaskStatus::Done,
                ..task_record("FOO-0001")
            }],
            "Active task not found: FOO-0001",
        ),
    ] {
        let store = InMemoryStore::default().with_project("foo", records);
        let error = plan_session::execute(
            &command(None),
            &store,
            &pool,
            &HomeDirectory::new("/home/dev".into()),
            &clients,
        )
        .await
        .unwrap_err();
        assert!(matches!(error, plan_session::PlanSessionError::FindTask(_)));
        assert_eq!(error.to_string(), expected);
    }
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn planning_accepts_unlinked_active_tasks_with_their_authored_content(
    pool: sqlx::SqlitePool,
) {
    insert_project(&pool, "FOO", "foo", "/work/foo", "/tasks/foo", false).await;
    let record = TaskRecord {
        placement: None,
        source: "authored task content".into(),
        ..task_record("FOO-0001")
    };
    let store = InMemoryStore::default().with_project("foo", vec![record]);
    let clients = SessionPlanningClients::new(AgentStub, ExistingProjectDirectory);
    let planned = plan(&command(None), &store, &pool, &clients).await.unwrap();
    assert!(
        planned
            .plan
            .launch
            .prompt
            .as_ref()
            .contains("authored task content")
    );
    assert_eq!(planned.plan.launch.task_ids, command(None).task_ids);
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn unknown_project_id_preserves_the_resolution_error(pool: sqlx::SqlitePool) {
    let clients = SessionPlanningClients::new(AgentStub, ExistingProjectDirectory);
    let mut command = command(None);
    command.task_ids = SessionTaskIds::try_new(["XYZ-0001".parse().unwrap()]).unwrap();
    let error = plan_session::execute(
        &command,
        &InMemoryStore::default(),
        &pool,
        &HomeDirectory::new("/home/dev".into()),
        &clients,
    )
    .await
    .unwrap_err();
    assert!(matches!(error, plan_session::PlanSessionError::FindTask(_)));
    assert_eq!(
        error.to_string(),
        "Unknown project ID `XYZ` for task XYZ-0001"
    );
}
