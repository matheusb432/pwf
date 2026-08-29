use std::{
    convert::Infallible,
    net::Ipv4Addr,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::Context as _;
use pwf_client::{
    ClientError, PwfClient,
    task::{Confirmation, ConfirmationPrompt},
    v1::{
        self, Agent, DispatchMode, DispatchSessionOutcome, IndexSection, PlanSessionIntent,
        ProjectStatusFilter, RemovedTaskOutcomeKind, ReopenedTaskOutcome, SessionEffort,
        TaskReadFormat, add_task_request, note_service_client::NoteServiceClient,
        task_service_client::TaskServiceClient,
    },
};
use pwf_local_auth::{
    CapabilityToken, LocalAuth, PublishedEndpoint, ServerEndpoint, ServerInstanceId,
};
use pwf_models::project::HomeDirectory;
use pwf_server::{AppState, ServerLifecycle, ServerState, serve};
use tokio::sync::{mpsc, oneshot, watch};
use tokio_stream::{StreamExt as _, wrappers::ReceiverStream};
use tonic::{Code, Request, Status, transport::Channel};
use tonic_health::{
    ServingStatus,
    pb::{HealthCheckRequest, health_client::HealthClient},
};
use tonic_reflection::pb::v1::{
    ServerReflectionRequest, server_reflection_client::ServerReflectionClient,
    server_reflection_request::MessageRequest, server_reflection_response::MessageResponse,
};

const TEST_TIMEOUT: Duration = Duration::from_secs(5);

struct TestServer {
    root: tempfile::TempDir,
    token: CapabilityToken,
    endpoint: ServerEndpoint,
    client: PwfClient,
    shutdown: Option<oneshot::Sender<()>>,
    lifecycle: watch::Receiver<ServerState>,
    task: Option<tokio::task::JoinHandle<anyhow::Result<()>>>,
    _published: PublishedEndpoint,
}

impl TestServer {
    async fn start(shutdown_grace_period: Duration) -> anyhow::Result<Self> {
        let root = tempfile::tempdir()?;
        let database_path = root.path().join("pwf.sqlite3");
        let migration_pool = pwf_infra::database::build_migration_pool(&database_path).await?;
        pwf_infra::database::migrate_database(&migration_pool).await?;
        migration_pool.close().await;
        let pool = pwf_infra::database::build_pool(&database_path).await?;
        let home_path = root.path().join("home");
        std::fs::create_dir_all(&home_path)?;
        let state = AppState::new(pool, HomeDirectory::new(home_path));

        let auth = LocalAuth::from_data_root(root.path().join("auth"))?;
        let token = auth.load_or_create_server_token()?;
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let endpoint =
            ServerEndpoint::try_new(listener.local_addr()?, ServerInstanceId::generate())?;
        let published = auth.publish_endpoint(endpoint.clone())?;
        let (shutdown_sender, shutdown_receiver) = oneshot::channel();
        let (lifecycle_handle, mut lifecycle) = ServerLifecycle::channel();
        let task = tokio::spawn(serve(
            listener,
            async move {
                let _ = shutdown_receiver.await;
            },
            shutdown_grace_period,
            token.clone(),
            state,
            lifecycle_handle,
        ));
        wait_for_state(&mut lifecycle, ServerState::Serving).await?;
        let client = PwfClient::connect(&auth).await?;

        Ok(Self {
            root,
            token,
            endpoint,
            client,
            shutdown: Some(shutdown_sender),
            lifecycle,
            task: Some(task),
            _published: published,
        })
    }

    async fn add_project_and_task(&self) -> anyhow::Result<String> {
        let project_path = self.root.path().join("project");
        let tasks_path = self.root.path().join("notes").join("foo-bar");
        std::fs::create_dir_all(&project_path)?;
        std::fs::create_dir_all(&tasks_path)?;
        std::fs::write(
            tasks_path.join("foo-bar.md"),
            "---\nid: foo\ntitle: foo-bar\n---\n",
        )?;

        let project = self
            .client
            .project()
            .add_project(v1::AddProjectRequest {
                fields: Some(v1::ProjectFields {
                    id: "FOO".to_string(),
                    title: "foo-bar".to_string(),
                    source_kind: "directory".to_string(),
                    source_value: project_path.to_string_lossy().into_owned(),
                    tasks_kind: "directory".to_string(),
                    tasks_path: tasks_path.to_string_lossy().into_owned(),
                }),
            })
            .await?;
        assert_eq!(project.id, "FOO");

        let task_id = self.add_task("transport task").await?;
        assert_eq!(task_id, "FOO-0001");
        Ok(task_id)
    }

    async fn add_task(&self, title: &str) -> anyhow::Result<String> {
        let task = self
            .client
            .task()
            .add_task(v1::AddTaskRequest {
                project_selector: "foo-bar".to_string(),
                prompt: Some(add_task_request::Prompt::Structured(
                    v1::StructuredTaskPrompt {
                        title: title.to_string(),
                        lanes: Some(v1::TaskLanes {
                            goals: vec!["exercise the real server".to_string()],
                            context: Vec::new(),
                            constraints: Vec::new(),
                            done_when: Vec::new(),
                        }),
                    },
                )),
                index_section: IndexSection::General as i32,
                blocked_by: Vec::new(),
                effort: None,
                tags: Vec::new(),
            })
            .await?;
        Ok(task.id)
    }

    async fn channel(&self) -> anyhow::Result<Channel> {
        Ok(
            tonic::transport::Endpoint::from_shared(format!("http://{}", self.endpoint.address()))?
                .connect()
                .await?,
        )
    }

    async fn open_remove_confirmation(
        &self,
        task_id: &str,
    ) -> anyhow::Result<(
        mpsc::Sender<v1::RemoveTaskRequest>,
        tonic::Streaming<v1::RemoveTaskResponse>,
    )> {
        let (sender, receiver) = mpsc::channel(2);
        sender
            .send(v1::RemoveTaskRequest {
                value: Some(v1::remove_task_request::Value::Start(v1::RemoveTaskStart {
                    id: task_id.to_string(),
                })),
            })
            .await?;
        let mut stream = TaskServiceClient::new(self.channel().await?)
            .remove_task(authenticated(
                Request::new(ReceiverStream::new(receiver)),
                &self.token,
            )?)
            .await?
            .into_inner();
        let preflight = stream
            .message()
            .await?
            .context("remove preflight response is missing")?;
        assert!(matches!(
            preflight.value,
            Some(v1::remove_task_response::Value::Preflight(_))
        ));
        Ok((sender, stream))
    }

    fn begin_shutdown(&mut self) -> anyhow::Result<()> {
        let shutdown = self
            .shutdown
            .take()
            .context("server shutdown sender is missing")?;
        shutdown
            .send(())
            .map_err(|()| anyhow::anyhow!("server shutdown receiver is closed"))
    }

    async fn wait_for(&mut self, state: ServerState) -> anyhow::Result<()> {
        wait_for_state(&mut self.lifecycle, state).await
    }

    async fn finish(mut self) -> anyhow::Result<()> {
        if self.shutdown.is_some() {
            self.begin_shutdown()?;
        }
        let task = self.task.take().context("server task is missing")?;
        let outcome = tokio::time::timeout(TEST_TIMEOUT, task).await?;
        outcome??;
        assert_eq!(*self.lifecycle.borrow(), ServerState::Stopped);
        Ok(())
    }
}

#[derive(Clone)]
struct RecordingPrompt {
    confirmed: bool,
    seen: Arc<Mutex<Vec<&'static str>>>,
}

impl RecordingPrompt {
    fn new(confirmed: bool) -> Self {
        Self {
            confirmed,
            seen: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn seen(&self) -> Vec<&'static str> {
        with_seen(&self.seen, |seen| seen.clone())
    }
}

impl ConfirmationPrompt for RecordingPrompt {
    type Error = Infallible;

    fn confirm(&self, confirmation: &Confirmation) -> Result<bool, Self::Error> {
        let operation = match confirmation {
            Confirmation::RemoveTask(_) => "remove",
            Confirmation::ReopenTask(_) => "reopen",
            Confirmation::DispatchSession(_) => "session",
        };
        with_seen(&self.seen, |seen| seen.push(operation));
        Ok(self.confirmed)
    }
}

fn with_seen<T>(
    seen: &Mutex<Vec<&'static str>>,
    operation: impl FnOnce(&mut Vec<&'static str>) -> T,
) -> T {
    let mut seen = match seen.lock() {
        Ok(seen) => seen,
        Err(poisoned) => poisoned.into_inner(),
    };
    operation(&mut seen)
}

#[tokio::test]
#[allow(
    clippy::too_many_lines,
    reason = "one real-server lifecycle verifies the related operation and confirmation invariants"
)]
async fn generated_client_preserves_operations_statuses_and_confirmation_flows()
-> anyhow::Result<()> {
    let server = TestServer::start(Duration::from_secs(2)).await?;
    let task_id = server.add_project_and_task().await?;
    let second_task_id = server.add_task("second transport task").await?;

    let invalid = server
        .client
        .project()
        .get_project(v1::GetProjectRequest {
            id: String::new(),
            status: ProjectStatusFilter::ActiveOnly as i32,
        })
        .await
        .unwrap_err();
    assert_eq!(rpc_status(invalid).code(), Code::InvalidArgument);

    let missing = server
        .client
        .project()
        .get_project(v1::GetProjectRequest {
            id: "BAR".to_string(),
            status: ProjectStatusFilter::ActiveOnly as i32,
        })
        .await
        .unwrap_err();
    assert_eq!(rpc_status(missing).code(), Code::NotFound);

    let remove_prompt = RecordingPrompt::new(false);
    let remove_result = server
        .client
        .task()
        .remove_task(
            v1::RemoveTaskStart {
                id: task_id.clone(),
            },
            remove_prompt.clone(),
        )
        .await
        .unwrap();
    assert_eq!(
        RemovedTaskOutcomeKind::try_from(remove_result.outcome).ok(),
        Some(RemovedTaskOutcomeKind::Aborted)
    );
    assert_eq!(remove_prompt.seen(), ["remove"]);

    let session_prompt = RecordingPrompt::new(false);
    let mut request = session_request(&task_id);
    request.task_ids = vec![second_task_id.clone(), task_id.clone()];
    let session_result = server
        .client
        .task()
        .dispatch_session(request, session_prompt.clone())
        .await
        .unwrap();
    assert_eq!(
        DispatchSessionOutcome::try_from(session_result.outcome).ok(),
        Some(DispatchSessionOutcome::Aborted)
    );
    assert_eq!(session_result.task_ids, [second_task_id, task_id.clone()]);
    assert_eq!(session_result.session_name, "foo1,foo2");
    assert_eq!(session_prompt.seen(), ["session"]);

    server
        .client
        .task()
        .complete_task(v1::CompleteTaskRequest {
            id: task_id.clone(),
            report: None,
            commits: vec!["a..b".to_string()],
            review: false,
        })
        .await
        .unwrap();
    let reopen_prompt = RecordingPrompt::new(false);
    let declined = server
        .client
        .task()
        .reopen_task(
            v1::ReopenTaskStart {
                id: task_id.clone(),
            },
            reopen_prompt.clone(),
        )
        .await
        .unwrap();
    assert_eq!(
        ReopenedTaskOutcome::try_from(declined.outcome).ok(),
        Some(ReopenedTaskOutcome::Aborted)
    );
    assert_eq!(reopen_prompt.seen(), ["reopen"]);

    let reopened = server
        .client
        .task()
        .reopen_task(
            v1::ReopenTaskStart {
                id: task_id.clone(),
            },
            RecordingPrompt::new(true),
        )
        .await
        .unwrap();
    assert_eq!(
        ReopenedTaskOutcome::try_from(reopened.outcome).ok(),
        Some(ReopenedTaskOutcome::Reopened)
    );
    let read = server
        .client
        .task()
        .get_task(v1::GetTaskRequest {
            id: task_id,
            output: TaskReadFormat::Path as i32,
        })
        .await
        .unwrap();
    assert!(matches!(
        read.value,
        Some(v1::get_task_response::Value::Path(_))
    ));

    server.finish().await
}

#[tokio::test]
async fn closing_confirmation_stream_before_decision_cancels_without_removing_task()
-> anyhow::Result<()> {
    let server = TestServer::start(Duration::from_secs(2)).await?;
    let task_id = server.add_project_and_task().await?;
    let (sender, mut remove) = server.open_remove_confirmation(&task_id).await?;

    drop(sender);

    let status = remove.message().await.unwrap_err();
    assert_eq!(status.code(), Code::Cancelled);
    server
        .client
        .task()
        .get_task(v1::GetTaskRequest {
            id: task_id,
            output: TaskReadFormat::Path as i32,
        })
        .await
        .unwrap();

    server.finish().await
}

#[tokio::test]
async fn health_and_reflection_require_authentication_and_requests_are_bounded()
-> anyhow::Result<()> {
    let server = TestServer::start(Duration::from_secs(2)).await?;
    server.add_project_and_task().await?;
    let channel = server.channel().await?;

    let unauthenticated = HealthClient::new(channel.clone())
        .check(HealthCheckRequest {
            service: String::new(),
        })
        .await
        .unwrap_err();
    assert_eq!(unauthenticated.code(), Code::Unauthenticated);

    let reflection_request = ServerReflectionRequest {
        host: String::new(),
        message_request: Some(MessageRequest::FileContainingSymbol(
            "pwf.v1.ProjectService".to_string(),
        )),
    };
    let request = authenticated(
        Request::new(tokio_stream::once(reflection_request)),
        &server.token,
    )?;
    let mut reflection = ServerReflectionClient::new(channel.clone())
        .server_reflection_info(request)
        .await
        .unwrap()
        .into_inner();
    let response = reflection
        .next()
        .await
        .unwrap()
        .unwrap()
        .message_response
        .unwrap();
    let MessageResponse::FileDescriptorResponse(descriptors) = response else {
        anyhow::bail!("reflection returned the wrong response kind");
    };
    assert!(
        descriptors
            .file_descriptor_proto
            .iter()
            .any(|descriptor| !descriptor.is_empty())
    );

    let oversized = NoteServiceClient::new(channel)
        .add_note(authenticated(
            Request::new(v1::AddNoteRequest {
                project_selector: "foo-bar".to_string(),
                title: "oversized".to_string(),
                content: "x".repeat(70 * 1024),
                why: None,
                domain: None,
                tags: Vec::new(),
                sources: Vec::new(),
                verified: None,
                date: None,
            }),
            &server.token,
        )?)
        .await
        .unwrap_err();
    assert_eq!(oversized.code(), Code::OutOfRange);

    server.finish().await
}

#[tokio::test]
async fn shutdown_publishes_not_serving_and_bounds_an_unanswered_stream() -> anyhow::Result<()> {
    let mut server = TestServer::start(Duration::from_millis(100)).await?;
    let task_id = server.add_project_and_task().await?;
    let channel = server.channel().await?;

    let mut health = HealthClient::new(channel.clone())
        .watch(authenticated(
            Request::new(HealthCheckRequest {
                service: String::new(),
            }),
            &server.token,
        )?)
        .await
        .unwrap()
        .into_inner();
    assert_eq!(
        health.message().await.unwrap().unwrap().status,
        ServingStatus::Serving as i32
    );

    let (_sender, _remove) = server.open_remove_confirmation(&task_id).await?;

    server.begin_shutdown()?;
    server.wait_for(ServerState::NotServing).await?;
    assert_eq!(
        health.message().await.unwrap().unwrap().status,
        ServingStatus::NotServing as i32
    );
    server.finish().await
}

fn session_request(task_id: &str) -> v1::PlanSessionRequest {
    v1::PlanSessionRequest {
        task_ids: vec![task_id.to_string()],
        intent: PlanSessionIntent::Dispatch as i32,
        pushed_prompt: None,
        mode: DispatchMode::Inline as i32,
        directives: Some(v1::LaunchDirectives::default()),
        agent: Agent::Codex as i32,
        model_override: None,
        effort: SessionEffort::High as i32,
        environment: std::collections::HashMap::new(),
    }
}

fn rpc_status(error: ClientError) -> Status {
    let ClientError::Rpc(status) = error;
    status
}

fn authenticated<T>(
    mut request: Request<T>,
    token: &CapabilityToken,
) -> anyhow::Result<Request<T>> {
    let authorization = format!("Bearer {}", token.expose_secret()).parse()?;
    request
        .metadata_mut()
        .insert("authorization", authorization);
    Ok(request)
}

async fn wait_for_state(
    lifecycle: &mut watch::Receiver<ServerState>,
    expected: ServerState,
) -> anyhow::Result<()> {
    tokio::time::timeout(TEST_TIMEOUT, observe_state(lifecycle, expected)).await??;
    Ok(())
}

async fn observe_state(
    lifecycle: &mut watch::Receiver<ServerState>,
    expected: ServerState,
) -> anyhow::Result<()> {
    loop {
        if *lifecycle.borrow_and_update() == expected {
            return Ok(());
        }
        lifecycle.changed().await?;
    }
}
