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
        self, Agent, DispatchMode, ProjectStatusFilter, SessionEffort, TaskReadFormat,
        delete_note_result, note_service_client::NoteServiceClient,
        session_service_client::SessionServiceClient,
        settings_service_client::SettingsServiceClient, task_service_client::TaskServiceClient,
    },
    task::{TaskDagEdge, TaskDagNode},
};
use pwf_server::ServerState;
use pwf_wire::pb::{delete_task_result, dispatched_session, get_task_response, reopen_task_result};
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
async fn v1_get_user_settings_returns_validated_task_status_colors() -> anyhow::Result<()> {
    let server = TestServer::start(TEST_TIMEOUT).await?;
    std::fs::write(
        server.root.path().join("config.toml"),
        "[colors]\nactive = \"#ff8700\"\n",
    )?;

    let response = SettingsServiceClient::new(server.channel().await?)
        .get_user_settings(Request::new(pb::GetUserSettingsRequest {}))
        .await?
        .into_inner();
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
    assert_eq!(colors.done, None);
    assert_eq!(colors.cancelled, None);
    Ok(())
}

#[tokio::test]
async fn v1_get_user_settings_rejects_invalid_config_with_its_path_and_cause() -> anyhow::Result<()>
{
    let server = TestServer::start(TEST_TIMEOUT).await?;
    let config_path = server.root.path().join("config.toml");
    std::fs::write(&config_path, "[colors]\nactive = \"#fff\"\n")?;

    let status = SettingsServiceClient::new(server.channel().await?)
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
    assert!(status.message().contains("`colors.active` is invalid"));
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
    assert_eq!(task_data(&server, &absent).await?.priority, None);

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
                project_selector: "foo-bar".to_string(),
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
                request_id: String::new(),
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
                    request_id: "transport-open-remove".to_string(),
                })),
            })
            .await?;
        let mut stream = TaskServiceClient::new(self.channel().await?)
            .delete_task(Request::new(ReceiverStream::new(receiver)))
            .await?
            .into_inner();
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
                    request_id: "transport-open-reopen".to_string(),
                })),
            })
            .await?;
        let mut stream = TaskServiceClient::new(self.channel().await?)
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
        let mut stream = SessionServiceClient::new(self.channel().await?)
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
        request_id: String::new(),
    }
}

fn task_list_request(
    number: Option<u64>,
    priority: Option<pb::PriorityTier>,
) -> pb::ListTasksRequest {
    pb::ListTasksRequest {
        project_selector: Some("foo-bar".to_string()),
        scope: Some(pb::list_tasks_request::Scope::All(pb::AllTaskSections {})),
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

async fn task_data(server: &TestServer, task_id: &str) -> anyhow::Result<Box<pb::TaskData>> {
    let read = server
        .client
        .task()
        .get_task(pb::GetTaskRequest {
            id: task_id.to_string(),
            output: TaskReadFormat::Data as i32,
        })
        .await?;
    let Some(get_task_response::Value::Data(data)) = read.value else {
        anyhow::bail!("task data response is missing");
    };
    Ok(data)
}

#[tokio::test]
async fn v1_create_task_returns_only_the_new_identifier() -> anyhow::Result<()> {
    let server = TestServer::start(Duration::from_secs(2)).await?;
    server.add_project_and_task().await?;

    let response = pb::task_service_client::TaskServiceClient::new(server.channel().await?)
        .create_task(Request::new(pb::CreateTaskRequest {
            project_selector: "foo-bar".to_string(),
            prompt: Some(pb::create_task_request::Prompt::Structured(
                pb::StructuredTaskPrompt {
                    title: "minimal response".to_string(),
                    lanes: Some(pb::TaskLanes {
                        goals: vec!["return only the task ID".to_string()],
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
            request_id: "transport-create-minimal-response".to_string(),
        }))
        .await?
        .into_inner();

    assert_eq!(
        response,
        pb::CreateTaskResponse {
            id: "FOO-0002".to_string()
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
            project_selector: "foo-bar".to_string(),
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
            request_id: "transport-create-dag-dependent".to_string(),
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
async fn v1_request_ids_replay_mutations_without_duplicate_effects() -> anyhow::Result<()> {
    let server = TestServer::start(Duration::from_secs(2)).await?;
    let original_id = server.add_project_and_task().await?;
    let create = pb::CreateTaskRequest {
        project_selector: "foo-bar".to_string(),
        prompt: Some(pb::create_task_request::Prompt::Structured(
            pb::StructuredTaskPrompt {
                title: "replayable task".to_string(),
                lanes: Some(pb::TaskLanes {
                    goals: vec!["exercise durable request identity".to_string()],
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
        request_id: "transport-replay-create".to_string(),
    };

    let first = server.client.task().create_task(create.clone()).await?;
    let replayed = server.client.task().create_task(create.clone()).await?;
    assert_eq!(first.id, "FOO-0002");
    assert_eq!(replayed.id, first.id);

    let mut conflicting = create;
    conflicting.tags = vec!["changed".to_string()];
    let conflict = server
        .client
        .task()
        .create_task(conflicting)
        .await
        .unwrap_err();
    assert_eq!(rpc_status(conflict)?.code(), Code::AlreadyExists);

    let append = pb::UpdateTaskRequest {
        id: first.id.clone(),
        content: Some(pb::TaskContentEdit {
            content: Some(pb::task_content_edit::Content::Append(
                pb::AppendTaskPrompt {
                    title: None,
                    prompt: "additional /c replayed context".to_string(),
                },
            )),
        }),
        blocked_by: None,
        effort: None,
        tags: None,
        priority: None,
        expected_revision: None,
        request_id: "transport-replay-update".to_string(),
    };
    server.client.task().update_task(append.clone()).await?;
    server.client.task().update_task(append).await?;
    let markdown = server
        .client
        .task()
        .get_task(pb::GetTaskRequest {
            id: first.id,
            output: TaskReadFormat::Markdown as i32,
        })
        .await?;
    let Some(get_task_response::Value::Markdown(markdown)) = markdown.value else {
        anyhow::bail!("replayed task markdown response is missing");
    };
    assert_eq!(markdown.matches("replayed context").count(), 1);

    let complete = pb::CompleteTaskRequest {
        id: original_id,
        report: None,
        commits: Vec::new(),
        expected_revision: None,
        request_id: "transport-replay-complete".to_string(),
    };
    let completed = server.client.task().complete_task(complete.clone()).await?;
    let completed_replay = server.client.task().complete_task(complete).await?;
    assert_eq!(completed_replay, completed);

    let mut list = task_list_request(None, None);
    list.status = Some(pb::TaskStatusFilter::All as i32);
    list.page_size = 256;
    let listed = server.client.task().list_tasks(list).await?;
    assert_eq!(listed.tasks.len(), 2, "replays must not create extra tasks");

    server.finish().await
}

#[tokio::test]
async fn v1_confirmed_mutation_replays_skip_the_second_prompt() -> anyhow::Result<()> {
    let server = TestServer::start(Duration::from_secs(2)).await?;
    let task_id = server.add_project_and_task().await?;
    let deleted_id = server.add_task("delete replay target").await?;
    let delete = pb::DeleteTaskStart {
        id: deleted_id,
        request_id: "transport-replay-delete".to_string(),
    };
    let deleted = server
        .client
        .task()
        .delete_task(delete.clone(), RecordingPrompt::new(true))
        .await
        .unwrap();
    assert!(matches!(
        deleted.outcome,
        Some(delete_task_result::Outcome::Deleted(_))
    ));
    let delete_replay_prompt = RecordingPrompt::new(false);
    let deleted_replay = server
        .client
        .task()
        .delete_task(delete, delete_replay_prompt.clone())
        .await
        .unwrap();
    assert!(matches!(
        deleted_replay.outcome,
        Some(delete_task_result::Outcome::Deleted(_))
    ));
    assert!(delete_replay_prompt.seen().is_empty());

    server
        .client
        .task()
        .complete_task(pb::CompleteTaskRequest {
            id: task_id.clone(),
            report: None,
            commits: Vec::new(),
            expected_revision: None,
            request_id: "transport-close-before-reopen-replay".to_string(),
        })
        .await?;
    let reopen = pb::ReopenTaskStart {
        id: task_id,
        request_id: "transport-replay-reopen".to_string(),
    };
    let reopened = server
        .client
        .task()
        .reopen_task(reopen.clone(), RecordingPrompt::new(true))
        .await
        .unwrap();
    assert!(matches!(
        reopened.outcome,
        Some(reopen_task_result::Outcome::Reopened(_))
    ));
    let reopen_replay_prompt = RecordingPrompt::new(false);
    let reopened_replay = server
        .client
        .task()
        .reopen_task(reopen, reopen_replay_prompt.clone())
        .await
        .unwrap();
    assert!(matches!(
        reopened_replay.outcome,
        Some(reopen_task_result::Outcome::Reopened(_))
    ));
    assert!(reopen_replay_prompt.seen().is_empty());

    server.finish().await
}

#[tokio::test]
async fn v1_task_revisions_support_conditional_empty_updates() -> anyhow::Result<()> {
    let server = TestServer::start(Duration::from_secs(2)).await?;
    let task_id = server.add_project_and_task().await?;
    let first = server
        .client
        .task()
        .get_task(pb::GetTaskRequest {
            id: task_id.clone(),
            output: TaskReadFormat::Data as i32,
        })
        .await?;
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
            request_id: "transport-conditional-update".to_string(),
        })
        .await?;
    assert_eq!(response, pb::UpdateTaskResponse {});

    let updated = server
        .client
        .task()
        .get_task(pb::GetTaskRequest {
            id: task_id.clone(),
            output: TaskReadFormat::Data as i32,
        })
        .await?;
    assert_ne!(updated.revision, first.revision);
    let Some(get_task_response::Value::Data(data)) = updated.value else {
        anyhow::bail!("updated task data response is missing");
    };
    assert_eq!(data.priority, Some(pb::PriorityTier::High as i32));

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
            request_id: "transport-stale-update".to_string(),
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
        task.project_path.clear();
        task.location = None;
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
    let data = task_data(&server, &task_id).await?;
    assert_eq!(data.priority, Some(pb::PriorityTier::Medium as i32));

    server
        .client
        .task()
        .update_task(priority_update(
            &task_id,
            Some(pb::priority_edit::Operation::Clear(pb::ClearField {})),
        ))
        .await?;
    let data = task_data(&server, &task_id).await?;
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
            project_selector: "foo-bar".to_string(),
            prompt: Some(pb::create_task_request::Prompt::Shorthand(
                "bounded collection".to_string(),
            )),
            blocked_by: Vec::new(),
            effort: None,
            tags: (0..65).map(|index| format!("tag-{index}")).collect(),
            priority: None,
            request_id: String::new(),
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
        id: "FOO".to_string(),
        source_value: Some(pb::StringFieldUpdate {
            operation: Some(pb::string_field_update::Operation::Update(
                source_value.clone(),
            )),
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
    assert_eq!(project.source_value, source_value);

    let invalid = server
        .client
        .project()
        .update_project(pb::UpdateProjectRequest {
            id: "FOO".to_string(),
            source_value: Some(pb::StringFieldUpdate {
                operation: Some(pb::string_field_update::Operation::Clear(pb::ClearField {})),
            }),
        })
        .await
        .unwrap_err();
    assert_eq!(rpc_status(invalid)?.code(), Code::InvalidArgument);

    let missing = server
        .client
        .project()
        .update_project(pb::UpdateProjectRequest {
            id: "MISS".to_string(),
            source_value: Some(pb::StringFieldUpdate {
                operation: Some(pb::string_field_update::Operation::Update(
                    "/work/missing".to_string(),
                )),
            }),
        })
        .await
        .unwrap_err();
    assert_eq!(rpc_status(missing)?.code(), Code::NotFound);

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
                request_id: String::new(),
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
                request_id: String::new(),
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
        .get_task(pb::GetTaskRequest {
            id: second_task_id,
            output: TaskReadFormat::Path as i32,
        })
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
            project_selector: "foo-bar".to_string(),
            title: "transport note".to_string(),
            content: "Exercise note confirmation over the real transport.".to_string(),
            domain: None,
            tags: Vec::new(),
            sources: Vec::new(),
            verified: None,
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
                project_selector: "foo-bar".to_string(),
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
            project_selector: "foo-bar".to_string(),
            limit_kind: pb::NoteListLimitKind::Default as i32,
            limit: 0,
        })
        .await?;
    assert_eq!(listed.notes.len(), 1);

    let accept_prompt = RecordingPrompt::new(true);
    let deleted = server
        .client
        .note()
        .delete_note(
            pb::DeleteNoteStart {
                project_selector: "foo-bar".to_string(),
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
            project_selector: "foo-bar".to_string(),
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
            request_id: String::new(),
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
                request_id: String::new(),
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
                request_id: String::new(),
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
        .get_task(pb::GetTaskRequest {
            id: task_id,
            output: TaskReadFormat::Path as i32,
        })
        .await
        .unwrap();
    assert!(matches!(
        read.value,
        Some(get_task_response::Value::Path(_))
    ));

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
        task_data(&server, &task_id).await?.priority,
        Some(pb::PriorityTier::Medium as i32)
    );
    assert!(
        !server
            .root
            .path()
            .join("notes/.trash")
            .join(format!("{task_id}.md"))
            .exists()
    );
    assert!(
        std::fs::read_to_string(server.root.path().join("notes/foo-bar/foo-bar.md"))?
            .contains(&task_id)
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
            request_id: String::new(),
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
    assert!(
        std::fs::read_to_string(server.root.path().join("notes/foo-bar/foo-bar.md"))?
            .contains(&format!("- [x] [[{task_id}]]"))
    );
    server
        .client
        .task()
        .get_task(pb::GetTaskRequest {
            id: created,
            output: TaskReadFormat::Path as i32,
        })
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
        task_data(&server, &task_id).await?.priority,
        Some(pb::PriorityTier::Medium as i32)
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
        .get_task(pb::GetTaskRequest {
            id: task_id,
            output: TaskReadFormat::Path as i32,
        })
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

    let oversized = NoteServiceClient::new(channel)
        .add_note(Request::new(pb::AddNoteRequest {
            project_selector: "foo-bar".to_string(),
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
        mode: DispatchMode::Inline as i32,
        directives: Some(pb::LaunchDirectives::default()),
        agent: Agent::Codex as i32,
        model_override: None,
        effort: SessionEffort::High as i32,
        environment: std::collections::HashMap::new(),
    }
}

fn rpc_status(error: ClientError) -> anyhow::Result<Status> {
    match error {
        ClientError::Rpc(status) => Ok(status),
        ClientError::InvalidTaskDagResponse(error) => Err(anyhow::Error::new(error)
            .context("expected an RPC status but received an invalid DAG response")),
    }
}

async fn reflection_response(
    channel: &Channel,
    message_request: MessageRequest,
) -> anyhow::Result<MessageResponse> {
    let request = Request::new(tokio_stream::once(ServerReflectionRequest {
        host: String::new(),
        message_request: Some(message_request),
    }));
    let mut reflection = ServerReflectionClient::new(channel.clone())
        .server_reflection_info(request)
        .await?
        .into_inner();
    reflection
        .next()
        .await
        .context("reflection response stream is empty")??
        .message_response
        .context("reflection response message is missing")
}
