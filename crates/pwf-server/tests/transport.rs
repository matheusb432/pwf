use std::{
    collections::BTreeSet,
    convert::Infallible,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::Context as _;
use pwf_client::{
    ClientError,
    confirmation::{Confirmation, ConfirmationPrompt},
    pb::{
        self, Agent, ProjectStatusFilter, SessionEffort, delete_note_result,
        note_service_client::NoteServiceClient, session_service_client::SessionServiceClient,
        settings_service_client::SettingsServiceClient, task_service_client::TaskServiceClient,
    },
    task::{TaskDagEdge, TaskDagNode},
};
use pwf_server::ServerState;
use pwf_wire::pb::{delete_task_result, dispatched_session, reopen_task_result};
use tokio::sync::mpsc;
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

#[path = "support/server.rs"]
mod server;

use server::TestServer;

const TEST_TIMEOUT: Duration = Duration::from_secs(5);

#[tokio::test]
async fn connecting_does_not_require_a_health_rpc() -> anyhow::Result<()> {
    use pwf_local_transport::{LocalEndpoint, LocalListener};
    let directory = tempfile::tempdir()?;
    let endpoint = LocalEndpoint::from_root(directory.path().join("runtime"))?;
    let listener = LocalListener::bind(&endpoint, TEST_TIMEOUT).await?;
    let (incoming, _ownership) = listener.into_parts();
    let (reporter, health) = tonic_health::server::health_reporter();
    reporter
        .set_service_status("", ServingStatus::NotServing)
        .await;
    let (shutdown, stopped) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(
        tonic::transport::Server::builder()
            .add_service(health)
            .serve_with_incoming_shutdown(incoming, async {
                let _ = stopped.await;
            }),
    );
    let client = pwf_client::PwfClient::connect(&endpoint).await?;
    assert!(!client.health().await?.serving);
    let error = client
        .project()
        .list_projects(pb::ListProjectsRequest::default())
        .await
        .unwrap_err();
    assert_eq!(rpc_status(error)?.code(), Code::Unimplemented);
    let _ = shutdown.send(());
    tokio::time::timeout(TEST_TIMEOUT, server).await???;
    Ok(())
}

#[tokio::test]
async fn release_metadata_rejects_unmatched_mutations_before_execution() -> anyhow::Result<()> {
    let server = TestServer::start(TEST_TIMEOUT).await?;
    server.add_project_and_task().await?;
    for version in [None, Some("999.0.0")] {
        let mut request = Request::new(pb::AddNoteRequest {
            project_id: "FOO".into(),
            title: "must not be created".into(),
            content: "rejected before mutation".into(),
            ..Default::default()
        });
        if let Some(version) = version {
            request
                .metadata_mut()
                .insert("pwf-client-version", version.parse()?);
        }
        let status = NoteServiceClient::new(server.channel().await?)
            .add_note(request)
            .await
            .unwrap_err();
        assert_eq!(status.code(), Code::FailedPrecondition);
        assert_eq!(
            status
                .metadata()
                .get("pwf-server-version")
                .context("missing server version")?,
            env!("CARGO_PKG_VERSION")
        );
    }
    let notes = server
        .client
        .note()
        .list_notes(pb::ListNotesRequest {
            project_id: "FOO".into(),
            limit_kind: pb::NoteListLimitKind::Unlimited as i32,
            limit: 0,
        })
        .await?;
    assert!(notes.notes.is_empty());
    let healthy = HealthClient::new(server.channel().await?)
        .check(HealthCheckRequest {
            service: String::new(),
        })
        .await?;
    assert_eq!(
        healthy
            .metadata()
            .get("pwf-server-version")
            .context("health has no server version")?,
        env!("CARGO_PKG_VERSION")
    );
    server.finish().await
}

#[tokio::test]
async fn v1_get_user_settings_returns_validated_scoped_colors() -> anyhow::Result<()> {
    let server = TestServer::start(TEST_TIMEOUT).await?;
    std::fs::write(
        server.root.path().join("config.toml"),
        "[colors.task]\nactive = \"#ff8700\"\n[colors.project]\npaused = \"#010203\"\n[colors.note]\nverified = \"#040506\"\n",
    )?;

    let response =
        SettingsServiceClient::with_interceptor(server.channel().await?, server::ReleaseRequest)
            .get_user_settings(Request::new(pb::GetUserSettingsRequest {}))
            .await?;
    assert_eq!(
        response
            .metadata()
            .get("pwf-server-version")
            .context("missing response version")?,
        env!("CARGO_PKG_VERSION")
    );
    let response = response.into_inner();
    let colors = response
        .task_status_colors
        .context("settings response is missing task status colors")?;

    assert_eq!(
        colors.active,
        Some(pb::RgbColor {
            red: 255,
            green: 135,
            blue: 0,
        })
    );
    assert_eq!(
        colors.done,
        Some(pb::RgbColor {
            red: 163,
            green: 230,
            blue: 53
        })
    );
    assert_eq!(
        colors.cancelled,
        Some(pb::RgbColor {
            red: 255,
            green: 107,
            blue: 138
        })
    );
    assert_eq!(
        response
            .project_status_colors
            .context("missing project colors")?
            .paused,
        Some(pb::RgbColor {
            red: 1,
            green: 2,
            blue: 3
        })
    );
    assert_eq!(
        response
            .note_status_colors
            .context("missing note colors")?
            .verified,
        Some(pb::RgbColor {
            red: 4,
            green: 5,
            blue: 6
        })
    );
    server.finish().await
}

#[tokio::test]
async fn v1_get_user_settings_rejects_invalid_config_with_its_path_and_cause() -> anyhow::Result<()>
{
    let server = TestServer::start(TEST_TIMEOUT).await?;
    let config_path = server.root.path().join("config.toml");
    std::fs::write(&config_path, "[colors.task]\nactive = \"#fff\"\n")?;

    let status =
        SettingsServiceClient::with_interceptor(server.channel().await?, server::ReleaseRequest)
            .get_user_settings(Request::new(pb::GetUserSettingsRequest {}))
            .await
            .unwrap_err();

    assert_eq!(status.code(), Code::FailedPrecondition);
    assert!(
        status
            .message()
            .contains(&config_path.display().to_string()),
        "{status}"
    );
    assert!(status.message().contains("`colors.task.active` is invalid"));
    assert!(status.message().contains("#RRGGBB"));
    Ok(())
}

#[tokio::test]
async fn list_defaults_apply_to_rpc_filters_order_and_pagination() -> anyhow::Result<()> {
    let server = TestServer::start(TEST_TIMEOUT).await?;
    let absent = server.add_project_and_task().await?;
    let low = server
        .add_task_with_priority("alpha", Some(pb::PriorityTier::Low))
        .await?;
    let highest = server
        .add_task_with_priority("zeta", Some(pb::PriorityTier::Highest))
        .await?;
    let config_path = server.root.path().join("config.toml");
    std::fs::write(
        &config_path,
        "default_priority = \"highest\"\ndefault_sort_order = \"priority\"\n",
    )?;
    let settings = server.client.settings().get_user_settings().await?;
    assert_eq!(settings.default_priority, pb::PriorityTier::Highest as i32);
    assert_eq!(
        settings.default_sort_order,
        Some(pb::OrderSpec {
            field: pb::OrderField::Priority as i32,
            direction: pb::OrderDirection::Desc as i32,
        })
    );
    let mut request = task_list_request(None, Some(pb::PriorityTier::Highest));
    request.page_size = 1;
    let first = server.client.task().list_tasks(request.clone()).await?;
    assert_eq!(first.tasks[0].id, highest);
    request.page_token = first.next_page_token;
    assert!(request.page_token.is_some());
    let second = server.client.task().list_tasks(request.clone()).await?;
    assert_eq!(second.tasks[0].id, absent);
    assert_eq!(
        second.tasks[0].priority,
        Some(pb::PriorityTier::Highest as i32)
    );
    assert_eq!(second.next_page_token, None);
    assert_eq!(task_record(&server, &absent).await?.priority, None);

    std::fs::write(
        &config_path,
        "default_priority = \"low\"\ndefault_sort_order = \"priority\"\n",
    )?;
    let changed = server.client.task().list_tasks(request).await.unwrap_err();
    assert_eq!(rpc_status(changed)?.code(), Code::InvalidArgument);
    let fresh = server
        .client
        .task()
        .list_tasks(task_list_request(None, Some(pb::PriorityTier::Highest)))
        .await?;
    assert_eq!(fresh.tasks.len(), 1);
    assert_eq!(fresh.tasks[0].id, highest);

    for (field, expected) in [
        (
            pb::OrderField::Title,
            vec![low.clone(), absent.clone(), highest.clone()],
        ),
        (
            pb::OrderField::Priority,
            vec![low.clone(), absent.clone(), highest.clone()],
        ),
        (
            pb::OrderField::Effort,
            vec![highest.clone(), low.clone(), absent.clone()],
        ),
    ] {
        let mut request = task_list_request(None, None);
        request.order = Some(pb::OrderSpec {
            field: field as i32,
            direction: pb::OrderDirection::Asc as i32,
        });
        let response = server.client.task().list_tasks(request).await?;
        assert_eq!(
            response
                .tasks
                .into_iter()
                .map(|task| task.id)
                .collect::<Vec<_>>(),
            expected
        );
    }
    server.finish().await
}

#[tokio::test]
async fn list_rejects_invalid_sort_fields_directions_and_configuration() -> anyhow::Result<()> {
    let server = TestServer::start(TEST_TIMEOUT).await?;
    server.add_project_and_task().await?;
    let config_path = server.root.path().join("config.toml");
    for order in [
        pb::OrderSpec {
            field: 999,
            direction: 1,
        },
        pb::OrderSpec {
            field: 1,
            direction: 999,
        },
    ] {
        let mut request = task_list_request(None, None);
        request.order = Some(order);
        let error = server.client.task().list_tasks(request).await.unwrap_err();
        assert_eq!(rpc_status(error)?.code(), Code::InvalidArgument);
    }
    std::fs::write(&config_path, "default_sort_order = \"wrong\"\n")?;
    let invalid = server
        .client
        .task()
        .list_tasks(task_list_request(None, None))
        .await
        .unwrap_err();
    let invalid = rpc_status(invalid)?;
    assert_eq!(invalid.code(), Code::FailedPrecondition);
    assert!(invalid.message().contains("default_sort_order"));
    server.finish().await
}

impl TestServer {
    async fn add_project_and_task(&self) -> anyhow::Result<String> {
        let project_path = self.root.path().join("project");
        let vault_path = self.root.path().join("notes");
        let tasks_path = vault_path.join("foo-bar");
        std::fs::create_dir_all(&project_path)?;
        std::fs::create_dir_all(&tasks_path)?;
        std::fs::create_dir_all(vault_path.join(".obsidian"))?;
        std::fs::write(
            tasks_path.join("foo-bar.md"),
            "---\nid: foo\ntitle: foo-bar\n---\n",
        )?;

        let project = self
            .client
            .project()
            .add_project(pb::AddProjectRequest {
                fields: Some(pb::ProjectFields {
                    snapshot_enabled: false,
                    obsidian_vault: None,
                    id: "FOO".to_string(),
                    title: "foo-bar".to_string(),
                    source_kind: Some("directory".to_string()),
                    source_value: Some(project_path.to_string_lossy().into_owned()),
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
        Ok(self.add_task_with_priority(title, None).await?)
    }

    async fn add_task_with_priority(
        &self,
        title: &str,
        priority: Option<pb::PriorityTier>,
    ) -> Result<String, ClientError> {
        let task = self
            .client
            .task()
            .create_task(pb::CreateTaskRequest {
                project_id: "FOO".to_string(),
                prompt: Some(pb::create_task_request::Prompt::Structured(
                    pb::StructuredTaskPrompt {
                        title: title.to_string(),
                        lanes: Some(pb::TaskLanes {
                            goals: vec!["exercise the real server".to_string()],
                            context: Vec::new(),
                            constraints: Vec::new(),
                            done_when: Vec::new(),
                        }),
                    },
                )),
                blocked_by: Vec::new(),
                effort: None,
                tags: Vec::new(),
                priority: priority.map(|priority| priority as i32),
            })
            .await?;
        Ok(task.id)
    }

    async fn open_remove_confirmation(
        &self,
        task_id: &str,
    ) -> anyhow::Result<(
        mpsc::Sender<pb::DeleteTaskRequest>,
        tonic::Streaming<pb::DeleteTaskResponse>,
    )> {
        let (sender, receiver) = mpsc::channel(2);
        sender
            .send(pb::DeleteTaskRequest {
                value: Some(pb::delete_task_request::Value::Start(pb::DeleteTaskStart {
                    id: task_id.to_string(),
                })),
            })
            .await?;
        let response =
            TaskServiceClient::with_interceptor(self.channel().await?, server::ReleaseRequest)
                .delete_task(Request::new(ReceiverStream::new(receiver)))
                .await?;
        assert_eq!(
            response
                .metadata()
                .get("pwf-server-version")
                .context("missing streaming version")?,
            env!("CARGO_PKG_VERSION")
        );
        let mut stream = response.into_inner();
        let preflight = stream
            .message()
            .await?
            .context("remove preflight response is missing")?;
        assert!(matches!(
            preflight.value,
            Some(pb::delete_task_response::Value::Preflight(_))
        ));
        Ok((sender, stream))
    }

    async fn open_reopen_confirmation(
        &self,
        task_id: &str,
    ) -> anyhow::Result<(
        mpsc::Sender<pb::ReopenTaskRequest>,
        tonic::Streaming<pb::ReopenTaskResponse>,
    )> {
        let (sender, receiver) = mpsc::channel(2);
        sender
            .send(pb::ReopenTaskRequest {
                value: Some(pb::reopen_task_request::Value::Start(pb::ReopenTaskStart {
                    id: task_id.to_string(),
                })),
            })
            .await?;
        let mut stream =
            TaskServiceClient::with_interceptor(self.channel().await?, server::ReleaseRequest)
                .reopen_task(Request::new(ReceiverStream::new(receiver)))
                .await?
                .into_inner();
        let preflight = stream
            .message()
            .await?
            .context("reopen preflight response is missing")?;
        assert!(matches!(
            preflight.value,
            Some(pb::reopen_task_response::Value::Preflight(_))
        ));
        Ok((sender, stream))
    }

    async fn open_session_confirmation(
        &self,
        task_id: &str,
    ) -> anyhow::Result<(
        mpsc::Sender<pb::DispatchSessionRequest>,
        tonic::Streaming<pb::DispatchSessionResponse>,
    )> {
        let (sender, receiver) = mpsc::channel(2);
        sender
            .send(pb::DispatchSessionRequest {
                value: Some(pb::dispatch_session_request::Value::Start(session_request(
                    task_id,
                ))),
            })
            .await?;
        let mut stream =
            SessionServiceClient::with_interceptor(self.channel().await?, server::ReleaseRequest)
                .dispatch_session(Request::new(ReceiverStream::new(receiver)))
                .await?
                .into_inner();
        let preflight = stream
            .message()
            .await?
            .context("session preflight response is missing")?;
        assert!(matches!(
            preflight.value,
            Some(pb::dispatch_session_response::Value::Preflight(_))
        ));
        Ok((sender, stream))
    }

    fn task_path(&self, task_id: &str) -> std::path::PathBuf {
        self.root
            .path()
            .join("notes/foo-bar")
            .join(format!("{task_id}.md"))
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
            Confirmation::DeleteNote(_) => "note-remove",
            Confirmation::DeleteTask(_) => "remove",
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

fn priority_update(
    task_id: &str,
    operation: Option<pb::priority_edit::Operation>,
) -> pb::UpdateTaskRequest {
    pb::UpdateTaskRequest {
        id: task_id.to_string(),
        content: None,
        blocked_by: None,
        effort: None,
        tags: None,
        priority: Some(pb::PriorityEdit { operation }),
        expected_revision: None,
    }
}

fn task_list_request(
    number: Option<u64>,
    priority: Option<pb::PriorityTier>,
) -> pb::ListTasksRequest {
    pb::ListTasksRequest {
        project_id: Some("FOO".to_string()),
        all: true,
        number,
        effort: None,
        tags: Vec::new(),
        order: None,
        status: Some(pb::TaskStatusFilter::Active as i32),
        detail: pb::ListDetail::Detailed as i32,
        priority: priority.map(|value| value as i32),
        page_size: 0,
        page_token: None,
    }
}

async fn task_record(server: &TestServer, task_id: &str) -> anyhow::Result<pb::TaskRecord> {
    let read = server
        .client
        .task()
        .get_task_record(pb::GetTaskRecordRequest {
            id: task_id.to_string(),
        })
        .await?;
    let Some(data) = read.record else {
        anyhow::bail!("task record response is missing");
    };
    Ok(data)
}

#[tokio::test]
async fn v1_create_task_returns_the_committed_task_summary() -> anyhow::Result<()> {
    let server = TestServer::start(Duration::from_secs(2)).await?;
    server.add_project_and_task().await?;

    let response = pb::task_service_client::TaskServiceClient::with_interceptor(
        server.channel().await?,
        server::ReleaseRequest,
    )
    .create_task(Request::new(pb::CreateTaskRequest {
        project_id: "FOO".to_string(),
        prompt: Some(pb::create_task_request::Prompt::Structured(
            pb::StructuredTaskPrompt {
                title: "task summary".to_string(),
                lanes: Some(pb::TaskLanes {
                    goals: vec!["return the committed task summary".to_string()],
                    context: Vec::new(),
                    constraints: Vec::new(),
                    done_when: Vec::new(),
                }),
            },
        )),
        blocked_by: Vec::new(),
        effort: None,
        tags: Vec::new(),
        priority: None,
    }))
    .await?
    .into_inner();

    assert_eq!(
        response,
        pb::CreateTaskResponse {
            id: "FOO-0002".to_string(),
            task: Some(pb::TaskMutationSummary {
                id: "FOO-0002".to_string(),
                title: "task summary".to_string(),
                status: pb::TaskStatus::Active as i32,
            }),
        }
    );
    server.finish().await
}

#[tokio::test]
async fn v1_get_task_dag_returns_typed_blocker_edges() -> anyhow::Result<()> {
    let server = TestServer::start(Duration::from_secs(2)).await?;
    let blocker_id = server.add_project_and_task().await?;
    let dependent = server
        .client
        .task()
        .create_task(pb::CreateTaskRequest {
            project_id: "FOO".to_string(),
            prompt: Some(pb::create_task_request::Prompt::Structured(
                pb::StructuredTaskPrompt {
                    title: "dependent task".to_string(),
                    lanes: Some(pb::TaskLanes {
                        goals: vec!["exercise the DAG endpoint".to_string()],
                        context: Vec::new(),
                        constraints: Vec::new(),
                        done_when: Vec::new(),
                    }),
                },
            )),
            blocked_by: vec![blocker_id],
            effort: None,
            tags: Vec::new(),
            priority: None,
        })
        .await?;

    let graph = server
        .client
        .task()
        .get_task_dag(pb::GetTaskDagRequest {
            id: dependent.id.clone(),
            depth: None,
            status: pb::TaskStatusFilter::All as i32,
            mode: pb::TaskDagMode::BlockedBy as i32,
        })
        .await?;

    assert_eq!(graph.root_id().as_ref(), dependent.id);
    assert_eq!(graph.nodes().len(), 2);
    assert!(matches!(
        graph.nodes()[0],
        TaskDagNode::Task { ref id, .. } if id.as_ref() == "FOO-0002"
    ));
    assert!(matches!(
        graph.nodes()[1],
        TaskDagNode::Task { ref id, .. } if id.as_ref() == "FOO-0001"
    ));
    assert_eq!(
        graph.edges(),
        [TaskDagEdge {
            blocker_node_index: 1,
            dependent_node_index: 0,
        }]
    );
    server.finish().await
}

#[tokio::test]
async fn v1_get_task_dag_rejects_unspecified_mode_and_zero_depth() -> anyhow::Result<()> {
    let server = TestServer::start(Duration::from_secs(2)).await?;

    let unspecified_mode = server
        .client
        .task()
        .get_task_dag(pb::GetTaskDagRequest {
            id: "FOO-0001".to_string(),
            depth: None,
            status: pb::TaskStatusFilter::All as i32,
            mode: pb::TaskDagMode::Unspecified as i32,
        })
        .await
        .unwrap_err();
    assert_eq!(rpc_status(unspecified_mode)?.code(), Code::InvalidArgument);

    let zero_depth = server
        .client
        .task()
        .get_task_dag(pb::GetTaskDagRequest {
            id: "FOO-0001".to_string(),
            depth: Some(0),
            status: pb::TaskStatusFilter::All as i32,
            mode: pb::TaskDagMode::BlockedBy as i32,
        })
        .await
        .unwrap_err();
    assert_eq!(rpc_status(zero_depth)?.code(), Code::InvalidArgument);

    server.finish().await
}

#[tokio::test]
async fn v1_repeated_requests_apply_each_create_and_append() -> anyhow::Result<()> {
    let server = TestServer::start(Duration::from_secs(2)).await?;
    let original_id = server.add_project_and_task().await?;
    let create = pb::CreateTaskRequest {
        project_id: "FOO".to_string(),
        prompt: Some(pb::create_task_request::Prompt::Structured(
            pb::StructuredTaskPrompt {
                title: "repeated task".to_string(),
                lanes: Some(pb::TaskLanes {
                    goals: vec!["exercise independent mutations".to_string()],
                    context: Vec::new(),
                    constraints: Vec::new(),
                    done_when: Vec::new(),
                }),
            },
        )),
        blocked_by: Vec::new(),
        effort: None,
        tags: Vec::new(),
        priority: None,
    };

    let first = server.client.task().create_task(create.clone()).await?;
    let repeated = server.client.task().create_task(create.clone()).await?;
    assert_eq!(first.id, "FOO-0002");
    assert_eq!(repeated.id, "FOO-0003");
    assert_ne!(repeated.id, first.id);

    let mut changed = create;
    changed.tags = vec!["changed".to_string()];
    let changed = server.client.task().create_task(changed).await?;
    assert_eq!(changed.id, "FOO-0004");

    let append = pb::UpdateTaskRequest {
        id: first.id.clone(),
        content: Some(pb::TaskContentEdit {
            content: Some(pb::task_content_edit::Content::Append(
                pb::AppendTaskPrompt {
                    title: None,
                    prompt: "additional /c repeated context".to_string(),
                },
            )),
        }),
        blocked_by: None,
        effort: None,
        tags: None,
        priority: None,
        expected_revision: None,
    };
    server.client.task().update_task(append.clone()).await?;
    server.client.task().update_task(append).await?;
    let markdown = server
        .client
        .task()
        .get_task_record(pb::GetTaskRecordRequest { id: first.id })
        .await?;
    let Some(markdown) = markdown.record else {
        anyhow::bail!("updated task markdown response is missing");
    };
    assert_eq!(markdown.source.matches("repeated context").count(), 2);

    let complete = pb::CompleteTaskRequest {
        id: original_id,
        report: None,
        commits: Vec::new(),
        expected_revision: None,
    };
    let completed = server.client.task().complete_task(complete.clone()).await?;
    assert_eq!(
        completed
            .task
            .context("completion summary is missing")?
            .status,
        pb::TaskStatus::Done as i32
    );
    let error = server
        .client
        .task()
        .complete_task(complete)
        .await
        .unwrap_err();
    assert_eq!(rpc_status(error)?.code(), Code::NotFound);

    let mut list = task_list_request(None, None);
    list.status = Some(pb::TaskStatusFilter::All as i32);
    list.page_size = 256;
    let listed = server.client.task().list_tasks(list).await?;
    assert_eq!(listed.tasks.len(), 4);

    server.finish().await
}

#[tokio::test]
async fn v1_repeated_confirmed_requests_observe_current_task_state() -> anyhow::Result<()> {
    let server = TestServer::start(Duration::from_secs(2)).await?;
    let task_id = server.add_project_and_task().await?;
    let deleted_id = server.add_task("delete target").await?;
    let delete = pb::DeleteTaskStart {
        id: deleted_id.clone(),
    };
    let prompt = RecordingPrompt::new(true);
    let deleted = server
        .client
        .task()
        .delete_task(delete.clone(), prompt.clone())
        .await?;
    let Some(delete_task_result::Outcome::Deleted(result)) = deleted.outcome else {
        anyhow::bail!("delete result is missing");
    };
    let summary = result.task.context("delete summary is missing")?;
    assert_eq!(summary.title, "delete target");
    assert_eq!(summary.status, pb::TaskStatus::Active as i32);
    assert_eq!(prompt.seen(), ["remove"]);
    let prompt = RecordingPrompt::new(true);
    let error = server
        .client
        .task()
        .delete_task(delete, prompt.clone())
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        pwf_client::confirmation::ConfirmedRequestError::Operation(status)
            if status.code() == Code::NotFound
    ));
    assert!(prompt.seen().is_empty());
    assert!(!server.task_path(&deleted_id).exists());
    let error = server
        .client
        .task()
        .get_task_record(pb::GetTaskRecordRequest { id: deleted_id })
        .await
        .unwrap_err();
    assert_eq!(rpc_status(error)?.code(), Code::NotFound);

    let complete = pb::CompleteTaskRequest {
        id: task_id.clone(),
        ..Default::default()
    };
    server.client.task().complete_task(complete.clone()).await?;
    let reopen = pb::ReopenTaskStart {
        id: task_id.clone(),
    };
    let prompt = RecordingPrompt::new(true);
    let reopened = server
        .client
        .task()
        .reopen_task(reopen.clone(), prompt.clone())
        .await?;
    assert!(matches!(
        reopened.outcome,
        Some(reopen_task_result::Outcome::Reopened(_))
    ));
    assert_eq!(prompt.seen(), ["reopen"]);
    let prompt = RecordingPrompt::new(false);
    let active = server
        .client
        .task()
        .reopen_task(reopen.clone(), prompt.clone())
        .await?;
    assert!(matches!(
        active.outcome,
        Some(reopen_task_result::Outcome::AlreadyActive(_))
    ));
    assert!(prompt.seen().is_empty());

    server.client.task().complete_task(complete).await?;
    let prompt = RecordingPrompt::new(false);
    let declined = server
        .client
        .task()
        .reopen_task(reopen, prompt.clone())
        .await?;
    assert!(matches!(
        declined.outcome,
        Some(reopen_task_result::Outcome::Aborted(_))
    ));
    assert_eq!(prompt.seen(), ["reopen"]);
    assert_eq!(
        task_record(&server, &task_id).await?.status,
        pb::TaskStatus::Done as i32
    );
    server.finish().await
}

#[tokio::test]
async fn v1_task_revisions_support_conditional_empty_updates() -> anyhow::Result<()> {
    let server = TestServer::start(Duration::from_secs(2)).await?;
    let task_id = server.add_project_and_task().await?;
    let first = server
        .client
        .task()
        .get_task_record(pb::GetTaskRecordRequest {
            id: task_id.clone(),
        })
        .await?;
    let first = first.record.context("missing task record")?;
    assert_eq!(first.revision.len(), 64);

    let response = server
        .client
        .task()
        .update_task(pb::UpdateTaskRequest {
            id: task_id.clone(),
            content: None,
            blocked_by: None,
            effort: None,
            tags: None,
            priority: Some(pb::PriorityEdit {
                operation: Some(pb::priority_edit::Operation::Set(
                    pb::PriorityTier::High as i32,
                )),
            }),
            expected_revision: Some(first.revision.clone()),
        })
        .await?;
    assert_eq!(
        response.task.as_ref().map(|task| task.id.as_str()),
        Some(task_id.as_str())
    );

    let updated = server
        .client
        .task()
        .get_task_record(pb::GetTaskRecordRequest {
            id: task_id.clone(),
        })
        .await?;
    let data = updated.record.context("missing updated task record")?;
    assert_ne!(data.revision, first.revision);
    assert_eq!(data.priority, Some("high".to_string()));

    let stale = server
        .client
        .task()
        .update_task(pb::UpdateTaskRequest {
            id: task_id,
            content: None,
            blocked_by: None,
            effort: Some(pb::EffortEdit {
                operation: Some(pb::effort_edit::Operation::Set(pb::EffortTier::Low as i32)),
            }),
            tags: None,
            priority: None,
            expected_revision: Some(first.revision),
        })
        .await
        .unwrap_err();
    assert_eq!(rpc_status(stale)?.code(), Code::Aborted);

    server.finish().await
}

#[tokio::test]
async fn v1_task_list_pages_are_bounded_and_query_bound() -> anyhow::Result<()> {
    let server = TestServer::start(Duration::from_secs(2)).await?;
    server.add_project_and_task().await?;
    server.add_task("second transport task").await?;
    server.add_task("third transport task").await?;
    let mut request = task_list_request(None, None);
    request.page_size = 2;
    request.order = Some(pb::OrderSpec {
        field: pb::OrderField::Id as i32,
        direction: pb::OrderDirection::Asc as i32,
    });

    let first = server.client.task().list_tasks(request.clone()).await?;
    assert_eq!(
        first
            .tasks
            .iter()
            .map(|task| task.id.as_str())
            .collect::<Vec<_>>(),
        ["FOO-0001", "FOO-0002"]
    );
    let token = first.next_page_token.context("first page must continue")?;

    server.add_task("added between task-list pages").await?;

    request.page_token = Some(token.clone());
    let second = server.client.task().list_tasks(request.clone()).await?;
    assert_eq!(
        second
            .tasks
            .iter()
            .map(|task| task.id.as_str())
            .collect::<Vec<_>>(),
        ["FOO-0003"]
    );
    assert_eq!(second.next_page_token, None);
    let replay = server.client.task().list_tasks(request.clone()).await?;
    assert_eq!(replay, second);
    let fresh = server
        .client
        .task()
        .list_tasks(task_list_request(None, None))
        .await?;
    assert_eq!(fresh.tasks.len(), 4);

    request.page_token = Some(token);
    request.order = Some(pb::OrderSpec {
        field: pb::OrderField::Id as i32,
        direction: pb::OrderDirection::Desc as i32,
    });
    let changed = server.client.task().list_tasks(request).await.unwrap_err();
    assert_eq!(rpc_status(changed)?.code(), Code::InvalidArgument);

    let mut oversized = task_list_request(None, None);
    oversized.page_size = 257;
    let oversized = server
        .client
        .task()
        .list_tasks(oversized)
        .await
        .unwrap_err();
    assert_eq!(rpc_status(oversized)?.code(), Code::InvalidArgument);

    server.finish().await
}

#[tokio::test]
async fn task_list_summary_omits_detailed_payload() -> anyhow::Result<()> {
    let server = TestServer::start(Duration::from_secs(2)).await?;
    server.add_project_and_task().await?;
    let mut request = task_list_request(None, None);
    let detailed = server.client.task().list_tasks(request.clone()).await?;
    request.detail = pb::ListDetail::Summary as i32;
    let summary = server.client.task().list_tasks(request).await?;
    let mut expected = detailed.tasks;
    for task in &mut expected {
        task.prompt.clear();
        task.project_path = None;
        task.note_path.clear();
        task.launch_issues.clear();
        task.blocked_by.clear();
        task.blocked_by_statuses.clear();
        task.blocked_by_issues.clear();
    }
    assert_eq!(summary.tasks, expected);
    server.finish().await
}

#[tokio::test]
async fn task_list_evicted_snapshot_returns_invalid_argument() -> anyhow::Result<()> {
    let server = TestServer::start(Duration::from_secs(2)).await?;
    server.add_project_and_task().await?;
    server.add_task("second task").await?;
    let mut request = task_list_request(None, None);
    request.page_size = 1;
    let first = server.client.task().list_tasks(request.clone()).await?;
    for _ in 0..16 {
        server.client.task().list_tasks(request.clone()).await?;
    }
    request.page_token = first.next_page_token;
    let error = server.client.task().list_tasks(request).await.unwrap_err();
    assert_eq!(rpc_status(error)?.code(), Code::InvalidArgument);
    server.finish().await
}

#[tokio::test]
async fn task_priority_round_trips_through_supported_rpcs() -> anyhow::Result<()> {
    let server = TestServer::start(Duration::from_secs(2)).await?;
    server.add_project_and_task().await?;
    let invalid = server
        .add_task_with_priority(
            "invalid priority transport task",
            Some(pb::PriorityTier::Unspecified),
        )
        .await
        .unwrap_err();
    assert_eq!(rpc_status(invalid)?.code(), Code::InvalidArgument);
    let task_id = server
        .add_task_with_priority("priority transport task", Some(pb::PriorityTier::Highest))
        .await?;
    assert_eq!(task_id, "FOO-0002");

    let invalid = server
        .client
        .task()
        .update_task(priority_update(&task_id, None))
        .await
        .unwrap_err();
    assert_eq!(rpc_status(invalid)?.code(), Code::InvalidArgument);

    let invalid = server
        .client
        .task()
        .list_tasks(task_list_request(Some(100_001), None))
        .await
        .unwrap_err();
    assert_eq!(rpc_status(invalid)?.code(), Code::InvalidArgument);

    let listed = server
        .client
        .task()
        .list_tasks(task_list_request(None, Some(pb::PriorityTier::Highest)))
        .await?;
    assert_eq!(listed.tasks.len(), 1);
    assert_eq!(listed.tasks[0].id, task_id);
    assert_eq!(
        listed.tasks[0].priority,
        Some(pb::PriorityTier::Highest as i32)
    );

    server
        .client
        .task()
        .update_task(priority_update(
            &task_id,
            Some(pb::priority_edit::Operation::Set(
                pb::PriorityTier::Medium as i32,
            )),
        ))
        .await?;
    let data = task_record(&server, &task_id).await?;
    assert_eq!(data.priority, Some("medium".to_string()));

    server
        .client
        .task()
        .update_task(priority_update(
            &task_id,
            Some(pb::priority_edit::Operation::Clear(pb::ClearField {})),
        ))
        .await?;
    let data = task_record(&server, &task_id).await?;
    assert_eq!(data.priority, None);

    server.finish().await
}

#[tokio::test]
async fn generated_client_maps_validation_and_not_found_statuses() -> anyhow::Result<()> {
    let server = TestServer::start(Duration::from_secs(2)).await?;
    server.add_project_and_task().await?;

    let invalid = server
        .client
        .project()
        .get_project(pb::GetProjectRequest {
            id: String::new(),
            status: ProjectStatusFilter::ActiveOnly as i32,
        })
        .await
        .unwrap_err();
    assert_eq!(rpc_status(invalid)?.code(), Code::InvalidArgument);

    let missing = server
        .client
        .project()
        .get_project(pb::GetProjectRequest {
            id: "BAR".to_string(),
            status: ProjectStatusFilter::ActiveOnly as i32,
        })
        .await
        .unwrap_err();
    assert_eq!(rpc_status(missing)?.code(), Code::NotFound);

    let oversized_collection = server
        .client
        .task()
        .create_task(pb::CreateTaskRequest {
            project_id: "FOO".to_string(),
            prompt: Some(pb::create_task_request::Prompt::Shorthand(
                "bounded collection".to_string(),
            )),
            blocked_by: Vec::new(),
            effort: None,
            tags: (0..65).map(|index| format!("tag-{index}")).collect(),
            priority: None,
        })
        .await
        .unwrap_err();
    assert_eq!(
        rpc_status(oversized_collection)?.code(),
        Code::InvalidArgument
    );

    server.finish().await
}

#[tokio::test]
async fn project_source_update_round_trips_through_the_generated_client() -> anyhow::Result<()> {
    let server = TestServer::start(Duration::from_secs(2)).await?;
    server.add_project_and_task().await?;
    let source_value = server.root.path().join("updated-project");
    let source_value = source_value.to_string_lossy().into_owned();
    let request = pb::UpdateProjectRequest {
        snapshot_enabled: None,
        obsidian_vault: None,
        id: "FOO".to_string(),
        source_value: Some(pb::StringPatchField {
            operation: Some(pb::string_patch_field::Operation::Set(source_value.clone())),
        }),
    };

    server
        .client
        .project()
        .update_project(request.clone())
        .await?;
    server.client.project().update_project(request).await?;

    let project = server
        .client
        .project()
        .get_project(pb::GetProjectRequest {
            id: "FOO".to_string(),
            status: ProjectStatusFilter::IncludingPaused as i32,
        })
        .await?;
    assert_eq!(project.source_value, Some(source_value));

    server
        .client
        .project()
        .update_project(pb::UpdateProjectRequest {
            snapshot_enabled: None,
            obsidian_vault: None,
            id: "FOO".to_string(),
            source_value: Some(pb::StringPatchField {
                operation: Some(pb::string_patch_field::Operation::Clear(pb::ClearField {})),
            }),
        })
        .await?;
    let cleared = server
        .client
        .project()
        .get_project(pb::GetProjectRequest {
            id: "FOO".to_string(),
            status: ProjectStatusFilter::IncludingPaused as i32,
        })
        .await?;
    assert_eq!(cleared.source_kind, None);
    assert_eq!(cleared.source_value, None);

    let missing = server
        .client
        .project()
        .update_project(pb::UpdateProjectRequest {
            snapshot_enabled: None,
            obsidian_vault: None,
            id: "MISS".to_string(),
            source_value: Some(pb::StringPatchField {
                operation: Some(pb::string_patch_field::Operation::Set(
                    "/work/missing".to_string(),
                )),
            }),
        })
        .await
        .unwrap_err();
    assert_eq!(rpc_status(missing)?.code(), Code::NotFound);

    server.finish().await
}

struct VaultPrompt {
    vault: String,
}
impl ConfirmationPrompt for VaultPrompt {
    type Error = std::io::Error;
    fn confirm(&self, confirmation: &Confirmation) -> Result<bool, Self::Error> {
        let Confirmation::DeleteTask(confirmation) = confirmation else {
            return Err(std::io::Error::other("expected delete preflight"));
        };
        assert_eq!(
            confirmation.obsidian_vault.as_deref(),
            Some(self.vault.as_str())
        );
        assert_eq!(
            confirmation.trash_folder.as_deref(),
            Some(
                std::path::Path::new(&self.vault)
                    .join(".trash")
                    .to_string_lossy()
                    .as_ref()
            )
        );
        Ok(false)
    }
}

#[tokio::test]
async fn registered_vault_round_trips_and_drives_delete_preflight() -> anyhow::Result<()> {
    let server = TestServer::start(TEST_TIMEOUT).await?;
    let task_id = server.add_project_and_task().await?;
    let vault = server.root.path().join("registered-vault");
    std::fs::create_dir_all(vault.join(".trash"))?;
    let vault = vault.to_string_lossy().into_owned();
    server
        .client
        .project()
        .update_project(pb::UpdateProjectRequest {
            snapshot_enabled: None,
            id: "FOO".into(),
            source_value: None,
            obsidian_vault: Some(pb::StringPatchField {
                operation: Some(pb::string_patch_field::Operation::Set(vault.clone())),
            }),
        })
        .await?;
    let project = server
        .client
        .project()
        .get_project(pb::GetProjectRequest {
            id: "FOO".into(),
            status: ProjectStatusFilter::IncludingPaused as i32,
        })
        .await?;
    assert_eq!(project.obsidian_vault.as_deref(), Some(vault.as_str()));
    let result = server
        .client
        .task()
        .delete_task(pb::DeleteTaskStart { id: task_id }, VaultPrompt { vault })
        .await?;
    assert!(matches!(
        result.outcome,
        Some(delete_task_result::Outcome::Aborted(_))
    ));
    server
        .client
        .project()
        .update_project(pb::UpdateProjectRequest {
            snapshot_enabled: None,
            id: "FOO".into(),
            source_value: None,
            obsidian_vault: Some(pb::StringPatchField {
                operation: Some(pb::string_patch_field::Operation::Clear(pb::ClearField {})),
            }),
        })
        .await?;
    let cleared = server
        .client
        .project()
        .get_project(pb::GetProjectRequest {
            id: "FOO".into(),
            status: ProjectStatusFilter::IncludingPaused as i32,
        })
        .await?;
    assert_eq!(cleared.obsidian_vault, None);
    assert_eq!(cleared.source_value, project.source_value);
    server.finish().await
}

#[tokio::test]
async fn generated_client_preserves_delete_and_session_confirmation_flows() -> anyhow::Result<()> {
    let server = TestServer::start(Duration::from_secs(2)).await?;
    let task_id = server.add_project_and_task().await?;
    let second_task_id = server.add_task("second transport task").await?;

    let remove_prompt = RecordingPrompt::new(false);
    let remove_result = server
        .client
        .task()
        .delete_task(
            pb::DeleteTaskStart {
                id: task_id.clone(),
            },
            remove_prompt.clone(),
        )
        .await
        .unwrap();
    assert!(matches!(
        remove_result.outcome,
        Some(delete_task_result::Outcome::Aborted(_))
    ));
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
    assert!(matches!(
        session_result.outcome,
        Some(dispatched_session::Outcome::Aborted(_))
    ));
    assert_eq!(session_prompt.seen(), ["session"]);

    let delete_result = server
        .client
        .task()
        .delete_task(
            pb::DeleteTaskStart {
                id: second_task_id.clone(),
            },
            RecordingPrompt::new(true),
        )
        .await
        .unwrap();
    assert!(matches!(
        delete_result.outcome,
        Some(delete_task_result::Outcome::Deleted(_))
    ));
    let missing = server
        .client
        .task()
        .get_task_record(pb::GetTaskRecordRequest { id: second_task_id })
        .await
        .unwrap_err();
    assert_eq!(rpc_status(missing)?.code(), Code::NotFound);

    server.finish().await
}

#[tokio::test]
async fn generated_client_preserves_note_removal_confirmation_flow() -> anyhow::Result<()> {
    let server = TestServer::start(Duration::from_secs(2)).await?;
    server.add_project_and_task().await?;
    let added = server
        .client
        .note()
        .add_note(pb::AddNoteRequest {
            project_id: "FOO".to_string(),
            title: "transport note".to_string(),
            content: "Exercise note confirmation over the real transport.".to_string(),
            domain: None,
            tags: Vec::new(),
            sources: Vec::new(),
            verified: Some("2026-09-08".to_string()),
            date: None,
        })
        .await?;
    assert_eq!(added.id, "FOO-NOTE-0001");

    let decline_prompt = RecordingPrompt::new(false);
    let declined = server
        .client
        .note()
        .delete_note(
            pb::DeleteNoteStart {
                project_id: "FOO".to_string(),
                selector: "1".to_string(),
            },
            decline_prompt.clone(),
        )
        .await
        .unwrap();
    assert!(matches!(
        declined.outcome,
        Some(delete_note_result::Outcome::Aborted(ref note))
            if note.id == "FOO-NOTE-0001"
    ));
    assert_eq!(decline_prompt.seen(), ["note-remove"]);
    let listed = server
        .client
        .note()
        .list_notes(pb::ListNotesRequest {
            project_id: "FOO".to_string(),
            limit_kind: pb::NoteListLimitKind::Default as i32,
            limit: 0,
        })
        .await?;
    assert_eq!(listed.notes.len(), 1);
    assert!(listed.notes[0].is_verified);

    let accept_prompt = RecordingPrompt::new(true);
    let deleted = server
        .client
        .note()
        .delete_note(
            pb::DeleteNoteStart {
                project_id: "FOO".to_string(),
                selector: "1".to_string(),
            },
            accept_prompt.clone(),
        )
        .await
        .unwrap();
    assert!(matches!(
        deleted.outcome,
        Some(delete_note_result::Outcome::Deleted(ref note))
            if note.id == "FOO-NOTE-0001"
                && note.project == "foo-bar"
                && note.title == "transport note"
    ));
    assert_eq!(accept_prompt.seen(), ["note-remove"]);
    let listed = server
        .client
        .note()
        .list_notes(pb::ListNotesRequest {
            project_id: "FOO".to_string(),
            limit_kind: pb::NoteListLimitKind::Default as i32,
            limit: 0,
        })
        .await?;
    assert!(listed.notes.is_empty());

    server.finish().await
}

#[tokio::test]
async fn generated_client_preserves_reopen_confirmation_flow() -> anyhow::Result<()> {
    let server = TestServer::start(Duration::from_secs(2)).await?;
    let task_id = server.add_project_and_task().await?;

    server
        .client
        .task()
        .complete_task(pb::CompleteTaskRequest {
            id: task_id.clone(),
            report: None,
            commits: vec!["a..b".to_string()],
            expected_revision: None,
        })
        .await
        .unwrap();
    let reopen_prompt = RecordingPrompt::new(false);
    let declined = server
        .client
        .task()
        .reopen_task(
            pb::ReopenTaskStart {
                id: task_id.clone(),
            },
            reopen_prompt.clone(),
        )
        .await
        .unwrap();
    assert!(matches!(
        declined.outcome,
        Some(reopen_task_result::Outcome::Aborted(_))
    ));
    assert_eq!(reopen_prompt.seen(), ["reopen"]);

    let reopened = server
        .client
        .task()
        .reopen_task(
            pb::ReopenTaskStart {
                id: task_id.clone(),
            },
            RecordingPrompt::new(true),
        )
        .await
        .unwrap();
    assert!(matches!(
        reopened.outcome,
        Some(reopen_task_result::Outcome::Reopened(_))
    ));
    let read = server
        .client
        .task()
        .get_task_record(pb::GetTaskRecordRequest { id: task_id })
        .await
        .unwrap();
    assert!(read.record.is_some());

    server.finish().await
}

#[tokio::test]
async fn remove_wait_does_not_hold_the_writer_lock_and_accepting_stale_preflight_aborts()
-> anyhow::Result<()> {
    let server = TestServer::start(Duration::from_secs(2)).await?;
    let task_id = server.add_project_and_task().await?;
    let (sender, mut remove) = server.open_remove_confirmation(&task_id).await?;

    tokio::time::timeout(
        TEST_TIMEOUT,
        server.client.task().update_task(priority_update(
            &task_id,
            Some(pb::priority_edit::Operation::Set(
                pb::PriorityTier::Medium as i32,
            )),
        )),
    )
    .await??;
    sender
        .send(pb::DeleteTaskRequest {
            value: Some(pb::delete_task_request::Value::Decision(
                pb::ConfirmationDecision { confirmed: true },
            )),
        })
        .await?;

    let status = remove.message().await.unwrap_err();

    assert_eq!(status.code(), Code::Aborted);
    assert_eq!(
        task_record(&server, &task_id).await?.priority,
        Some("medium".to_string())
    );
    assert!(
        !server
            .root
            .path()
            .join("notes/.trash")
            .join(format!("{task_id}.md"))
            .exists()
    );
    assert_eq!(
        std::fs::read_to_string(server.root.path().join("notes/foo-bar/foo-bar.md"))?,
        "---\nid: foo\ntitle: foo-bar\n---\n"
    );

    server.finish().await
}

#[tokio::test]
async fn reopen_wait_does_not_hold_the_writer_lock_and_accepting_stale_preflight_aborts()
-> anyhow::Result<()> {
    let server = TestServer::start(Duration::from_secs(2)).await?;
    let task_id = server.add_project_and_task().await?;
    server
        .client
        .task()
        .complete_task(pb::CompleteTaskRequest {
            id: task_id.clone(),
            report: None,
            commits: Vec::new(),
            expected_revision: None,
        })
        .await?;
    let (sender, mut reopen) = server.open_reopen_confirmation(&task_id).await?;

    let created = tokio::time::timeout(
        TEST_TIMEOUT,
        server.add_task("writer completed while reopen waited"),
    )
    .await??;
    let task_path = server.task_path(&task_id);
    let external = format!("{}\nexternal edit\n", std::fs::read_to_string(&task_path)?);
    std::fs::write(&task_path, &external)?;
    sender
        .send(pb::ReopenTaskRequest {
            value: Some(pb::reopen_task_request::Value::Decision(
                pb::ConfirmationDecision { confirmed: true },
            )),
        })
        .await?;

    let status = reopen.message().await.unwrap_err();

    assert_eq!(status.code(), Code::Aborted);
    assert_eq!(std::fs::read_to_string(task_path)?, external);
    assert_eq!(
        std::fs::read_to_string(server.root.path().join("notes/foo-bar/foo-bar.md"))?,
        "---\nid: foo\ntitle: foo-bar\n---\n"
    );
    server
        .client
        .task()
        .get_task_record(pb::GetTaskRecordRequest { id: created })
        .await?;

    server.finish().await
}

#[tokio::test]
async fn session_wait_does_not_hold_the_writer_lock_and_accepting_stale_preflight_aborts()
-> anyhow::Result<()> {
    let server = TestServer::start(Duration::from_secs(2)).await?;
    let task_id = server.add_project_and_task().await?;
    let (sender, mut session) = server.open_session_confirmation(&task_id).await?;

    tokio::time::timeout(
        TEST_TIMEOUT,
        server.client.task().update_task(priority_update(
            &task_id,
            Some(pb::priority_edit::Operation::Set(
                pb::PriorityTier::Medium as i32,
            )),
        )),
    )
    .await??;
    sender
        .send(pb::DispatchSessionRequest {
            value: Some(pb::dispatch_session_request::Value::Decision(
                pb::ConfirmationDecision { confirmed: true },
            )),
        })
        .await?;

    let status = session.message().await.unwrap_err();

    assert_eq!(status.code(), Code::Aborted);
    assert_eq!(
        task_record(&server, &task_id).await?.priority,
        Some("medium".to_string())
    );

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
        .get_task_record(pb::GetTaskRecordRequest { id: task_id })
        .await
        .unwrap();

    server.finish().await
}

#[tokio::test]
async fn health_and_reflection_use_local_ipc_and_requests_are_bounded() -> anyhow::Result<()> {
    let server = TestServer::start(Duration::from_secs(2)).await?;
    server.add_project_and_task().await?;
    let channel = server.channel().await?;

    let healthy = HealthClient::new(channel.clone())
        .check(HealthCheckRequest {
            service: String::new(),
        })
        .await?
        .into_inner();
    assert_eq!(healthy.status, ServingStatus::Serving as i32);

    let response =
        reflection_response(&channel, MessageRequest::ListServices(String::new())).await?;
    let MessageResponse::ListServicesResponse(services) = response else {
        anyhow::bail!("reflection returned the wrong service-list response kind");
    };
    let service_names = services
        .service
        .into_iter()
        .map(|service| service.name)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        service_names,
        BTreeSet::from([
            "grpc.health.v1.Health".to_string(),
            "grpc.reflection.v1.ServerReflection".to_string(),
            "pwf.v1.NoteService".to_string(),
            "pwf.v1.ProjectService".to_string(),
            "pwf.v1.SessionService".to_string(),
            "pwf.v1.SettingsService".to_string(),
            "pwf.v1.TaskService".to_string(),
        ])
    );

    let response = reflection_response(
        &channel,
        MessageRequest::FileContainingSymbol("pwf.v1.ProjectService".to_string()),
    )
    .await?;
    let MessageResponse::FileDescriptorResponse(descriptors) = response else {
        anyhow::bail!("reflection returned the wrong response kind");
    };
    assert!(
        descriptors
            .file_descriptor_proto
            .iter()
            .any(|descriptor| !descriptor.is_empty())
    );

    let oversized = NoteServiceClient::with_interceptor(channel, server::ReleaseRequest)
        .add_note(Request::new(pb::AddNoteRequest {
            project_id: "FOO".to_string(),
            title: "oversized".to_string(),
            content: "x".repeat(70 * 1024),
            domain: None,
            tags: Vec::new(),
            sources: Vec::new(),
            verified: None,
            date: None,
        }))
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
        .watch(Request::new(HealthCheckRequest {
            service: String::new(),
        }))
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

fn session_request(task_id: &str) -> pb::DispatchSessionStart {
    pb::DispatchSessionStart {
        task_ids: vec![task_id.to_string()],
        pushed_prompt: None,
        agent: Agent::Codex as i32,
        model_override: None,
        effort: SessionEffort::High as i32,
        environment: std::collections::HashMap::new(),
    }
}

fn rpc_status(error: ClientError) -> anyhow::Result<Status> {
    match error {
        ClientError::Rpc(status) => Ok(status),
        ClientError::InvalidTaskResponse(error) => Err(anyhow::Error::new(error)
            .context("expected an RPC status but received an invalid task response")),
        ClientError::InvalidTaskDagResponse(error) => Err(anyhow::Error::new(error)
            .context("expected an RPC status but received an invalid DAG response")),
    }
}

async fn reflection_response(
    channel: &Channel,
    message_request: MessageRequest,
) -> anyhow::Result<MessageResponse> {
    let mut request = Request::new(tokio_stream::once(ServerReflectionRequest {
        host: String::new(),
        message_request: Some(message_request),
    }));
    request
        .metadata_mut()
        .insert("pwf-client-version", "999.0.0".parse()?);
    let response = ServerReflectionClient::new(channel.clone())
        .server_reflection_info(request)
        .await?;
    assert_eq!(
        response
            .metadata()
            .get("pwf-server-version")
            .context("missing response version")?,
        env!("CARGO_PKG_VERSION")
    );
    let mut reflection = response.into_inner();
    reflection
        .next()
        .await
        .context("reflection response stream is empty")??
        .message_response
        .context("reflection response message is missing")
}

#[tokio::test]
async fn add_vault_project_resolves_defaults_and_preserves_add_project_errors() -> anyhow::Result<()>
{
    let server = TestServer::start(TEST_TIMEOUT).await?;
    let vault = server.root.path().join("vault");
    std::fs::create_dir_all(vault.join(".obsidian"))?;
    let request = pb::AddVaultProjectRequest {
        vault_path: vault.to_string_lossy().into_owned(),
        id: "FOO".to_string(),
        tasks_path: "some/path/foo".to_string(),
        title: None,
        source_path: None,
    };
    let created = server
        .client
        .project()
        .add_vault_project(request.clone())
        .await?;
    assert_eq!(created.id, "FOO");
    let project = server
        .client
        .project()
        .get_project(pb::GetProjectRequest {
            id: created.id,
            status: ProjectStatusFilter::ActiveOnly as i32,
        })
        .await?;
    assert_eq!(project.title, "foo");
    assert_eq!(project.source_value, None);
    assert_eq!(project.source_kind, None);
    assert!(
        project
            .tasks_path
            .ends_with(&std::path::Path::new("some/path/foo").display().to_string())
    );
    assert!(project.obsidian_vault.is_some());
    assert!(!vault.join("some/path/foo").exists());
    assert!(!vault.join(".trash").exists());
    let duplicate = server
        .client
        .project()
        .add_vault_project(request.clone())
        .await
        .unwrap_err();
    assert_eq!(rpc_status(duplicate)?.code(), Code::AlreadyExists);
    let malformed = server
        .client
        .project()
        .add_vault_project(pb::AddVaultProjectRequest {
            tasks_path: "../escape".to_string(),
            ..request.clone()
        })
        .await
        .unwrap_err();
    assert_eq!(rpc_status(malformed)?.code(), Code::InvalidArgument);
    std::fs::remove_dir(vault.join(".obsidian"))?;
    std::fs::write(vault.join(".obsidian"), "")?;
    let invalid = server
        .client
        .project()
        .add_vault_project(request)
        .await
        .unwrap_err();
    assert_eq!(rpc_status(invalid)?.code(), Code::FailedPrecondition);
    server.finish().await
}

#[tokio::test]
async fn project_operations_require_ids_and_report_missing_projects() -> anyhow::Result<()> {
    let server = TestServer::start(TEST_TIMEOUT).await?;
    server.add_project_and_task().await?;
    for (project_id, expected) in [("foo-bar", Code::InvalidArgument), ("MISS", Code::NotFound)] {
        let error = server
            .client
            .task()
            .create_task(pb::CreateTaskRequest {
                project_id: project_id.to_string(),
                prompt: Some(pb::create_task_request::Prompt::Structured(
                    pb::StructuredTaskPrompt {
                        title: "rejected task".to_string(),
                        lanes: Some(pb::TaskLanes {
                            goals: vec!["require an ID".to_string()],
                            ..Default::default()
                        }),
                    },
                )),
                ..Default::default()
            })
            .await
            .unwrap_err();
        assert_eq!(rpc_status(error)?.code(), expected);
        let error = server
            .client
            .task()
            .list_tasks(pb::ListTasksRequest {
                project_id: Some(project_id.to_string()),
                ..task_list_request(None, None)
            })
            .await
            .unwrap_err();
        assert_eq!(rpc_status(error)?.code(), expected);
        let error = server
            .client
            .note()
            .add_note(pb::AddNoteRequest {
                project_id: project_id.to_string(),
                title: "rejected note".to_string(),
                content: "require an ID".to_string(),
                ..Default::default()
            })
            .await
            .unwrap_err();
        assert_eq!(rpc_status(error)?.code(), expected);
        let error = server
            .client
            .note()
            .list_notes(pb::ListNotesRequest {
                project_id: project_id.to_string(),
                limit_kind: pb::NoteListLimitKind::Default as i32,
                limit: 0,
            })
            .await
            .unwrap_err();
        assert_eq!(rpc_status(error)?.code(), expected);
        let error = server
            .client
            .note()
            .update_note(pb::UpdateNoteRequest {
                project_id: project_id.to_string(),
                selector: "1".to_string(),
                title: Some("rejected edit".to_string()),
                ..Default::default()
            })
            .await
            .unwrap_err();
        assert_eq!(rpc_status(error)?.code(), expected);
        let prompt = RecordingPrompt::new(false);
        let error = server
            .client
            .note()
            .delete_note(
                pb::DeleteNoteStart {
                    project_id: project_id.to_string(),
                    selector: "1".to_string(),
                },
                prompt.clone(),
            )
            .await
            .unwrap_err();
        match error {
            pwf_client::confirmation::ConfirmedRequestError::Operation(status) => {
                assert_eq!(status.code(), expected);
            }
            pwf_client::confirmation::ConfirmedRequestError::Prompt(error) => match error {},
        }
        assert!(prompt.seen().is_empty());
    }
    assert_eq!(server.add_task("next task").await?, "FOO-0002");
    server.finish().await
}

#[tokio::test]
async fn get_task_record_preserves_raw_metadata_and_deleted_notes_are_not_found()
-> anyhow::Result<()> {
    let server = TestServer::start(TEST_TIMEOUT).await?;
    let id = server.add_project_and_task().await?;
    let path = server.root.path().join("notes/foo-bar/FOO-0001.md");
    let source = "---\nid: FOO-0001\ntitle: raw task\nstatus: active\ncreated_at: 2026-07-26T12:34:56Z\neffort: extreme\npriority: urgent\nblocked_by: bad links\n---\n\n  authored body  \n";
    std::fs::write(&path, source)?;
    let record = task_record(&server, &id).await?;
    assert_eq!(record.source, source);
    assert_eq!(record.effort.as_deref(), Some("extreme"));
    assert_eq!(record.priority.as_deref(), Some("urgent"));
    assert_eq!(record.created_at.as_deref(), Some("2026-07-26T12:34:56Z"));
    assert_eq!(record.locator, path.to_string_lossy());
    assert!(
        matches!(record.blocked_by.and_then(|state| state.value), Some(pb::stored_task_blocked_by::Value::Malformed(value)) if value.raw.contains("bad links"))
    );
    assert_eq!(task_record(&server, &id).await?.revision, record.revision);

    std::fs::remove_file(&path)?;
    let error = server
        .client
        .task()
        .get_task_record(pb::GetTaskRecordRequest { id })
        .await
        .unwrap_err();
    assert_eq!(rpc_status(error)?.code(), Code::NotFound);
    server.finish().await
}

#[tokio::test]
async fn get_task_returns_domain_values_and_classifies_invalid_records() -> anyhow::Result<()> {
    let server = TestServer::start(TEST_TIMEOUT).await?;
    let id = server.add_project_and_task().await?;
    let path = server.root.path().join("notes/foo-bar/FOO-0001.md");
    let source = "---\nid: FOO-0001\ntitle: Typed Task\nstatus: active\ncreated_at: 2026-07-26T12:34:56Z\neffort: high\npriority: highest\ntags: [rust, sqlite]\n---\n\n  authored body  \n";
    std::fs::write(&path, source)?;
    let task = server
        .client
        .task()
        .get_task(pb::GetTaskRequest { id: id.clone() })
        .await?;
    assert_eq!(task.title.as_ref(), "typed task");
    assert_eq!(task.effort, Some(pwf_models::task::EffortTier::High));
    assert_eq!(task.priority, Some(pwf_models::task::PriorityTier::Highest));
    assert_eq!(task.created_at.unwrap().to_string(), "2026-07-26T12:34:56Z");
    assert_eq!(
        task.revision.as_ref(),
        task_record(&server, &id).await?.revision
    );

    std::fs::write(&path, source.replace("effort: high", "effort: extreme"))?;
    let error = server
        .client
        .task()
        .get_task(pb::GetTaskRequest { id: id.clone() })
        .await
        .unwrap_err();
    assert_eq!(rpc_status(error)?.code(), Code::DataLoss);
    assert_eq!(
        task_record(&server, &id).await?.effort.as_deref(),
        Some("extreme")
    );

    std::fs::remove_file(&path)?;
    let error = server
        .client
        .task()
        .get_task(pb::GetTaskRequest { id })
        .await
        .unwrap_err();
    assert_eq!(rpc_status(error)?.code(), Code::NotFound);
    for id in ["FOO-9999", "MISS-0001"] {
        let error = server
            .client
            .task()
            .get_task(pb::GetTaskRequest { id: id.to_string() })
            .await
            .unwrap_err();
        assert_eq!(rpc_status(error)?.code(), Code::NotFound);
    }
    let error = server
        .client
        .task()
        .get_task(pb::GetTaskRequest {
            id: "invalid".to_string(),
        })
        .await
        .unwrap_err();
    assert_eq!(rpc_status(error)?.code(), Code::InvalidArgument);
    server.finish().await
}

#[tokio::test]
async fn clone_copies_into_an_explicit_project_and_requires_an_existing_source()
-> anyhow::Result<()> {
    let server = TestServer::start(TEST_TIMEOUT).await?;
    let source_id = server.add_project_and_task().await?;
    let destination = server.root.path().join("destination");
    std::fs::create_dir_all(&destination)?;
    server
        .client
        .project()
        .add_project(pb::AddProjectRequest {
            fields: Some(pb::ProjectFields {
                snapshot_enabled: false,
                id: "ALT".into(),
                title: "destination".into(),
                tasks_kind: "directory".into(),
                tasks_path: destination.to_string_lossy().into_owned(),
                ..Default::default()
            }),
        })
        .await?;
    server
        .client
        .task()
        .complete_task(pb::CompleteTaskRequest {
            id: source_id.clone(),
            commits: vec!["abc..def".into()],
            ..Default::default()
        })
        .await?;
    let source_path = server.root.path().join("notes/foo-bar/FOO-0001.md");
    let note = std::fs::read_to_string(&source_path)?;
    let (metadata, _) = note
        .split_once("\n---\n")
        .context("fixture frontmatter missing")?;
    std::fs::write(
        &source_path,
        format!("{metadata}\n---\n  literal /g text\r\n\r\nend  "),
    )?;
    let source = server
        .client
        .task()
        .get_task(pb::GetTaskRequest {
            id: source_id.clone(),
        })
        .await?;
    let request = pb::CloneTaskRequest {
        id: source_id.clone(),
        project_id: Some("ALT".into()),
    };
    let response = server.client.task().clone_task(request.clone()).await?;
    assert_eq!(response.id, "ALT-0001");
    let cloned = server
        .client
        .task()
        .get_task(pb::GetTaskRequest {
            id: response.id.clone(),
        })
        .await?;
    assert_eq!(cloned.title, source.title);
    assert_eq!(cloned.prompt, source.prompt);
    assert_eq!(cloned.status, pwf_models::task::TaskStatus::Active);
    assert!(cloned.completed_at.is_none());
    assert!(cloned.commits.is_none());
    let repeated = server.client.task().clone_task(request.clone()).await?;
    assert_eq!(repeated.id, "ALT-0002");
    std::fs::remove_file(&source_path)?;
    let error = server.client.task().clone_task(request).await.unwrap_err();
    assert_eq!(rpc_status(error)?.code(), Code::NotFound);
    server.finish().await
}

#[tokio::test]
async fn clone_reports_invalid_input_and_missing_sources() -> anyhow::Result<()> {
    let server = TestServer::start(TEST_TIMEOUT).await?;
    server.add_project_and_task().await?;
    for (id, project_id, code) in [
        ("invalid", None, Code::InvalidArgument),
        (
            "FOO-0001",
            Some("project-name".into()),
            Code::InvalidArgument,
        ),
        ("FOO-9999", None, Code::NotFound),
        ("FOO-0001", Some("MISS".into()), Code::NotFound),
    ] {
        let error = server
            .client
            .task()
            .clone_task(pb::CloneTaskRequest {
                id: id.into(),
                project_id,
            })
            .await
            .unwrap_err();
        assert_eq!(rpc_status(error)?.code(), code);
    }
    server.finish().await
}

#[tokio::test]
async fn project_resolution_returns_one_id_and_classifies_invalid_and_missing_selectors()
-> anyhow::Result<()> {
    let server = TestServer::start(TEST_TIMEOUT).await?;
    server.add_project_and_task().await?;
    let mut client = pb::project_service_client::ProjectServiceClient::with_interceptor(
        server.channel().await?,
        server::ReleaseRequest,
    );
    for selector in ["foo", "FOO", " Foo ", "FOO-BAR"] {
        let response = client
            .resolve_project(pb::ResolveProjectRequest {
                selector: selector.into(),
                status: ProjectStatusFilter::ActiveOnly as i32,
            })
            .await?;
        assert_eq!(response.into_inner().id, "FOO");
    }
    for (selector, status, expected) in [
        (
            " ",
            ProjectStatusFilter::ActiveOnly as i32,
            Code::InvalidArgument,
        ),
        (
            "foo",
            ProjectStatusFilter::Unspecified as i32,
            Code::InvalidArgument,
        ),
        ("foo", 999, Code::InvalidArgument),
        (
            "missing",
            ProjectStatusFilter::ActiveOnly as i32,
            Code::NotFound,
        ),
    ] {
        let error = client
            .resolve_project(pb::ResolveProjectRequest {
                selector: selector.into(),
                status,
            })
            .await
            .unwrap_err();
        assert_eq!(error.code(), expected);
    }
    server
        .client
        .project()
        .pause_project(pb::PauseProjectRequest { id: "FOO".into() })
        .await?;
    let error = client
        .resolve_project(pb::ResolveProjectRequest {
            selector: "foo".into(),
            status: ProjectStatusFilter::ActiveOnly as i32,
        })
        .await
        .unwrap_err();
    assert_eq!(error.code(), Code::NotFound);
    let response = client
        .resolve_project(pb::ResolveProjectRequest {
            selector: "foo".into(),
            status: ProjectStatusFilter::IncludingPaused as i32,
        })
        .await?;
    assert_eq!(response.into_inner().id, "FOO");
    server.finish().await
}

#[tokio::test]
async fn task_and_note_crud_ignore_missing_arbitrary_and_malformed_project_pages()
-> anyhow::Result<()> {
    for page_source in [
        None,
        Some("# Personal project page\n\n## Someday\n- [ ] [[FOO-9999]]\n- [[FOO-NOTE-9999]]\n"),
        Some("---\nid: [broken\n---\n\n## Broken task links\n- [ ] [[FOO-9999]]\n"),
    ] {
        let server = TestServer::start(TEST_TIMEOUT).await?;
        let original_id = server.add_project_and_task().await?;
        let directory = server.root.path().join("notes/foo-bar");
        let page = directory.join("foo-bar.md");
        match page_source {
            Some(source) => std::fs::write(&page, source)?,
            None => std::fs::remove_file(&page)?,
        }

        exercise_task_note_crud(&server, &original_id).await?;
        exercise_project_note_crud(&server, &directory).await?;

        match page_source {
            Some(source) => assert_eq!(std::fs::read(&page)?, source.as_bytes()),
            None => assert!(!page.exists()),
        }
        server.finish().await?;
    }
    Ok(())
}

async fn exercise_task_note_crud(server: &TestServer, original_id: &str) -> anyhow::Result<()> {
    let client = server.client.task();
    let id = server.add_task("independent task note").await?;
    assert_eq!(id, "FOO-0002");
    let task = client
        .get_task(pb::GetTaskRequest { id: id.clone() })
        .await?;
    assert_eq!(task.title.as_ref(), "independent task note");
    client
        .update_task(priority_update(
            &id,
            Some(pb::priority_edit::Operation::Set(
                pb::PriorityTier::Highest as i32,
            )),
        ))
        .await?;
    let record = task_record(server, &id).await?;
    assert_eq!(record.priority.as_deref(), Some("highest"));
    assert_eq!(record.locator, server.task_path(&id).to_string_lossy());
    let list = client.list_tasks(task_list_request(None, None)).await?;
    assert_eq!(
        list.tasks
            .iter()
            .map(|task| task.id.as_str())
            .collect::<Vec<_>>(),
        ["FOO-0002", "FOO-0001"]
    );
    assert_eq!(
        list.tasks[0].note_path,
        server.task_path(&id).to_string_lossy()
    );

    client
        .complete_task(pb::CompleteTaskRequest {
            id: id.clone(),
            ..Default::default()
        })
        .await?;
    assert_eq!(
        task_record(server, &id).await?.status,
        pb::TaskStatus::Done as i32
    );
    let reopened = client
        .reopen_task(
            pb::ReopenTaskStart { id: id.clone() },
            RecordingPrompt::new(true),
        )
        .await?;
    assert!(matches!(
        reopened.outcome,
        Some(reopen_task_result::Outcome::Reopened(_))
    ));
    client
        .cancel_task(pb::CancelTaskRequest {
            id: id.clone(),
            report: "obsolete work".into(),
            ..Default::default()
        })
        .await?;
    assert_eq!(
        task_record(server, &id).await?.status,
        pb::TaskStatus::Cancelled as i32
    );
    let deleted = client
        .delete_task(
            pb::DeleteTaskStart { id: id.clone() },
            RecordingPrompt::new(true),
        )
        .await?;
    assert!(matches!(
        deleted.outcome,
        Some(delete_task_result::Outcome::Deleted(_))
    ));
    assert!(!server.task_path(&id).exists());
    let missing = client
        .get_task_record(pb::GetTaskRecordRequest { id })
        .await
        .err()
        .context("deleted task unexpectedly remained available")?;
    assert_eq!(rpc_status(missing)?.code(), Code::NotFound);
    let list = client.list_tasks(task_list_request(None, None)).await?;
    assert_eq!(
        list.tasks
            .iter()
            .map(|task| task.id.as_str())
            .collect::<Vec<_>>(),
        [original_id]
    );
    Ok(())
}

async fn exercise_project_note_crud(
    server: &TestServer,
    directory: &std::path::Path,
) -> anyhow::Result<()> {
    let client = server.client.note();
    let note = client
        .add_note(pb::AddNoteRequest {
            project_id: "FOO".into(),
            title: "independent project note".into(),
            content: "authored evidence".into(),
            ..Default::default()
        })
        .await?;
    assert_eq!(note.id, "FOO-NOTE-0001");
    client
        .update_note(pb::UpdateNoteRequest {
            project_id: "FOO".into(),
            selector: "1".into(),
            title: Some("revised project note".into()),
            content: Some("revised evidence".into()),
            ..Default::default()
        })
        .await?;
    let list_request = pb::ListNotesRequest {
        project_id: "FOO".into(),
        limit_kind: pb::NoteListLimitKind::Default as i32,
        limit: 0,
    };
    let notes = client.list_notes(list_request.clone()).await?;
    assert_eq!(notes.notes.len(), 1);
    assert_eq!(notes.notes[0].id, note.id);
    assert_eq!(notes.notes[0].title, "revised project note");
    let note_path = directory.join(format!("{}.md", note.id));
    assert!(std::fs::read_to_string(&note_path)?.contains("revised evidence"));
    let deleted = client
        .delete_note(
            pb::DeleteNoteStart {
                project_id: "FOO".into(),
                selector: "1".into(),
            },
            RecordingPrompt::new(true),
        )
        .await?;
    assert!(matches!(
        deleted.outcome,
        Some(delete_note_result::Outcome::Deleted(_))
    ));
    assert!(!note_path.exists());
    assert!(client.list_notes(list_request).await?.notes.is_empty());
    Ok(())
}

#[tokio::test]
async fn serve_refreshes_snapshots_at_startup_and_joins_shutdown() -> anyhow::Result<()> {
    use pwf_infra::user_settings::TomlSettingsStore;
    use pwf_local_transport::{LocalEndpoint, LocalListener};
    use pwf_models::project::HomeDirectory;
    use pwf_server::{AppState, ServerLifecycle, serve};

    let root = tempfile::tempdir()?;
    let database = root.path().join("pwf.sqlite3");
    pwf_migrator::run(&database).await?;
    let pool = pwf_infra::database::build_pool(&database).await?;
    let directory = root.path().join("foo");
    std::fs::create_dir_all(&directory)?;
    sqlx::query(
        "INSERT INTO projects (id, title, tasks_kind, tasks_path, snapshot_enabled) VALUES ('FOO', 'foo', 'directory', ?, 1)",
    )
    .bind(directory.to_str().context("project path is not Unicode")?)
    .execute(&pool)
    .await?;
    std::fs::write(
        directory.join("FOO-0001.md"),
        "---\nid: FOO-0001\nstatus: active\ntitle: startup task\n---\n\nGenerate a startup snapshot.\n",
    )?;
    std::fs::write(directory.join("FOO-NOTE-0001.md"), "# Startup evidence\n")?;
    let snapshot = directory.join("pwf-index.md");
    assert!(!snapshot.exists());

    let state = AppState::new(
        pool,
        HomeDirectory::new(root.path().to_path_buf()),
        TomlSettingsStore::new(Some(root.path().join("config.toml"))),
    );
    let endpoint = LocalEndpoint::from_root(root.path().join("runtime"))?;
    let listener = LocalListener::bind(&endpoint, Duration::from_millis(250)).await?;
    let (shutdown, stopped) = tokio::sync::oneshot::channel();
    let (lifecycle, mut states) = ServerLifecycle::channel();
    let server = tokio::spawn(serve(
        listener,
        async move {
            let _ = stopped.await;
        },
        Duration::from_secs(1),
        state,
        lifecycle,
    ));
    let observed = tokio::time::timeout(
        Duration::from_secs(2),
        wait_for_startup_snapshot(&mut states, &snapshot),
    )
    .await;

    shutdown
        .send(())
        .map_err(|()| anyhow::anyhow!("server shutdown receiver is closed"))?;
    tokio::time::timeout(Duration::from_secs(2), server)
        .await
        .context("waiting for serve and its snapshot worker to stop")???;
    assert_eq!(*states.borrow(), ServerState::Stopped);
    let source = observed.context("startup snapshot was not generated within two seconds")??;
    assert_eq!(std::fs::read_to_string(snapshot)?, source);
    Ok(())
}

async fn wait_for_startup_snapshot(
    states: &mut tokio::sync::watch::Receiver<ServerState>,
    snapshot: &std::path::Path,
) -> anyhow::Result<String> {
    states
        .wait_for(|state| *state == ServerState::Serving)
        .await?;
    loop {
        if let Ok(source) = std::fs::read_to_string(snapshot)
            && source.contains("[[FOO-0001]]")
            && source.contains("[[FOO-NOTE-0001]]")
        {
            return Ok(source);
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
