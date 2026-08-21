#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "transport-test setup and assertions fail immediately with operation-specific context"
)]

use std::{
    convert::Infallible,
    net::Ipv4Addr,
    sync::{Arc, Mutex},
    time::Duration,
};

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
    async fn start(shutdown_grace_period: Duration) -> Self {
        let root = tempfile::tempdir().expect("create test server root");
        let database_path = root.path().join("pwf.sqlite3");
        let migration_pool = pwf_infra::database::build_migration_pool(&database_path)
            .await
            .expect("open migration database");
        pwf_infra::database::migrate_database(&migration_pool)
            .await
            .expect("migrate test database");
        migration_pool.close().await;
        let pool = pwf_infra::database::build_pool(&database_path)
            .await
            .expect("open application database");
        let home_path = root.path().join("home");
        std::fs::create_dir_all(&home_path).expect("create test home");
        let state = AppState::new(pool, HomeDirectory::new(home_path));

        let auth =
            LocalAuth::from_data_root(root.path().join("auth")).expect("create local auth root");
        let token = auth
            .load_or_create_server_token()
            .expect("provision test capability");
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind test server");
        let endpoint = ServerEndpoint::try_new(
            listener.local_addr().expect("read listener address"),
            ServerInstanceId::generate(),
        )
        .expect("create loopback endpoint");
        let published = auth
            .publish_endpoint(endpoint.clone())
            .expect("publish test endpoint");
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
        wait_for_state(&mut lifecycle, ServerState::Serving).await;
        let client = PwfClient::connect(&auth)
            .await
            .expect("connect generated client to test server");

        Self {
            root,
            token,
            endpoint,
            client,
            shutdown: Some(shutdown_sender),
            lifecycle,
            task: Some(task),
            _published: published,
        }
    }

    async fn add_project_and_task(&self) -> String {
        let project_path = self.root.path().join("project");
        let tasks_path = self.root.path().join("notes").join("foo-bar");
        std::fs::create_dir_all(&project_path).expect("create project source");
        std::fs::create_dir_all(&tasks_path).expect("create project task directory");
        std::fs::write(
            tasks_path.join("foo-bar.md"),
            "---\nid: foo\ntitle: foo-bar\n---\n",
        )
        .expect("write task index");

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
            .await
            .expect("add project through gRPC");
        assert_eq!(project.id, "FOO");

        let task = self
            .client
            .task()
            .add_task(v1::AddTaskRequest {
                project_selector: "foo-bar".to_string(),
                prompt: Some(add_task_request::Prompt::Structured(
                    v1::StructuredTaskPrompt {
                        title: "transport task".to_string(),
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
            .await
            .expect("add task through gRPC");
        assert_eq!(task.id, "FOO-0001");
        task.id
    }

    async fn channel(&self) -> Channel {
        tonic::transport::Endpoint::from_shared(format!("http://{}", self.endpoint.address()))
            .expect("valid test endpoint")
            .connect()
            .await
            .expect("connect raw generated client")
    }

    fn begin_shutdown(&mut self) {
        self.shutdown
            .take()
            .expect("shutdown is sent once")
            .send(())
            .expect("server receives shutdown");
    }

    async fn wait_for(&mut self, state: ServerState) {
        wait_for_state(&mut self.lifecycle, state).await;
    }

    async fn finish(mut self) {
        if self.shutdown.is_some() {
            self.begin_shutdown();
        }
        let task = self.task.take().expect("server task exists");
        tokio::time::timeout(TEST_TIMEOUT, task)
            .await
            .expect("server stops within test timeout")
            .expect("join test server")
            .expect("test server exits successfully");
        assert_eq!(*self.lifecycle.borrow(), ServerState::Stopped);
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
        self.seen.lock().expect("prompt lock").clone()
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
        self.seen.lock().expect("prompt lock").push(operation);
        Ok(self.confirmed)
    }
}

#[tokio::test]
#[allow(
    clippy::too_many_lines,
    reason = "one real-server lifecycle verifies the related operation and confirmation invariants"
)]
async fn generated_client_preserves_operations_statuses_and_confirmation_flows() {
    let server = TestServer::start(Duration::from_secs(2)).await;
    let task_id = server.add_project_and_task().await;

    let invalid = server
        .client
        .project()
        .get_project(v1::GetProjectRequest {
            id: String::new(),
            status: ProjectStatusFilter::ActiveOnly as i32,
        })
        .await
        .expect_err("empty project id is invalid");
    assert_eq!(rpc_status(invalid).code(), Code::InvalidArgument);

    let missing = server
        .client
        .project()
        .get_project(v1::GetProjectRequest {
            id: "BAR".to_string(),
            status: ProjectStatusFilter::ActiveOnly as i32,
        })
        .await
        .expect_err("unknown project is not found");
    assert_eq!(rpc_status(missing).code(), Code::NotFound);

    let remove_prompt = RecordingPrompt::new(false);
    let remove_result = server
        .client
        .task()
        .remove_task(
            v1::RemoveTaskRequest {
                id: task_id.clone(),
            },
            remove_prompt.clone(),
        )
        .await
        .expect("decline remove through stream");
    assert_eq!(
        RemovedTaskOutcomeKind::try_from(remove_result.outcome).ok(),
        Some(RemovedTaskOutcomeKind::Aborted)
    );
    assert_eq!(remove_prompt.seen(), ["remove"]);

    let session_prompt = RecordingPrompt::new(false);
    let session_result = server
        .client
        .task()
        .dispatch_session(session_request(&task_id), session_prompt.clone())
        .await
        .expect("decline session through stream");
    assert_eq!(
        DispatchSessionOutcome::try_from(session_result.outcome).ok(),
        Some(DispatchSessionOutcome::Aborted)
    );
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
        .expect("complete task through unary RPC");
    let reopen_prompt = RecordingPrompt::new(false);
    let declined = server
        .client
        .task()
        .reopen_task(
            v1::ReopenTaskRequest {
                id: task_id.clone(),
            },
            reopen_prompt.clone(),
        )
        .await
        .expect("decline reopen through stream");
    assert_eq!(
        ReopenedTaskOutcome::try_from(declined.outcome).ok(),
        Some(ReopenedTaskOutcome::Aborted)
    );
    assert_eq!(reopen_prompt.seen(), ["reopen"]);

    let reopened = server
        .client
        .task()
        .reopen_task(
            v1::ReopenTaskRequest {
                id: task_id.clone(),
            },
            RecordingPrompt::new(true),
        )
        .await
        .expect("confirm reopen through stream");
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
        .expect("read task after declined remove and confirmed reopen");
    assert!(matches!(read.value, Some(v1::task_read::Value::Path(_))));

    server.finish().await;
}

#[tokio::test]
async fn health_and_reflection_require_authentication_and_requests_are_bounded() {
    let server = TestServer::start(Duration::from_secs(2)).await;
    server.add_project_and_task().await;
    let channel = server.channel().await;

    let unauthenticated = HealthClient::new(channel.clone())
        .check(HealthCheckRequest {
            service: String::new(),
        })
        .await
        .expect_err("health rejects missing capability");
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
    );
    let mut reflection = ServerReflectionClient::new(channel.clone())
        .server_reflection_info(request)
        .await
        .expect("authenticated reflection request")
        .into_inner();
    let response = reflection
        .next()
        .await
        .expect("reflection response exists")
        .expect("reflection response succeeds")
        .message_response
        .expect("reflection response has a value");
    let MessageResponse::FileDescriptorResponse(descriptors) = response else {
        panic!("reflection returned the wrong response kind");
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
        ))
        .await
        .expect_err("server rejects a request beyond its message limit");
    assert_eq!(oversized.code(), Code::OutOfRange);

    server.finish().await;
}

#[tokio::test]
async fn shutdown_publishes_not_serving_and_bounds_an_unanswered_stream() {
    let mut server = TestServer::start(Duration::from_millis(100)).await;
    let task_id = server.add_project_and_task().await;
    let channel = server.channel().await;

    let mut health = HealthClient::new(channel.clone())
        .watch(authenticated(
            Request::new(HealthCheckRequest {
                service: String::new(),
            }),
            &server.token,
        ))
        .await
        .expect("open authenticated health watch")
        .into_inner();
    assert_eq!(
        health
            .message()
            .await
            .expect("read initial health")
            .expect("initial health exists")
            .status,
        ServingStatus::Serving as i32
    );

    let (sender, receiver) = mpsc::channel(2);
    sender
        .send(v1::RemoveTaskClientMessage {
            value: Some(v1::remove_task_client_message::Value::Start(
                v1::RemoveTaskRequest { id: task_id },
            )),
        })
        .await
        .expect("send remove start");
    let mut remove = TaskServiceClient::new(channel)
        .remove_task(authenticated(
            Request::new(ReceiverStream::new(receiver)),
            &server.token,
        ))
        .await
        .expect("open remove stream")
        .into_inner();
    let preflight = remove
        .message()
        .await
        .expect("read remove preflight")
        .expect("remove preflight exists");
    assert!(matches!(
        preflight.value,
        Some(v1::remove_task_server_message::Value::Preflight(_))
    ));

    server.begin_shutdown();
    server.wait_for(ServerState::NotServing).await;
    assert_eq!(
        health
            .message()
            .await
            .expect("read shutdown health")
            .expect("shutdown health exists")
            .status,
        ServingStatus::NotServing as i32
    );
    server.finish().await;
}

fn session_request(task_id: &str) -> v1::PlanSessionRequest {
    v1::PlanSessionRequest {
        task_id: task_id.to_string(),
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

fn authenticated<T>(mut request: Request<T>, token: &CapabilityToken) -> Request<T> {
    request.metadata_mut().insert(
        "authorization",
        format!("Bearer {}", token.expose_secret())
            .parse()
            .expect("capability is valid metadata"),
    );
    request
}

async fn wait_for_state(lifecycle: &mut watch::Receiver<ServerState>, expected: ServerState) {
    tokio::time::timeout(TEST_TIMEOUT, async {
        loop {
            if *lifecycle.borrow_and_update() == expected {
                return;
            }
            lifecycle
                .changed()
                .await
                .expect("server lifecycle remains observable");
        }
    })
    .await
    .unwrap_or_else(|_| panic!("server did not reach {expected:?}"));
}
