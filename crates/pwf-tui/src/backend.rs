use std::fmt::Write as _;
mod confirmation;
mod inspection;
mod load_records;
mod task_mutation;

use std::{path::PathBuf, time::Duration};

use anyhow::{Context as _, Result, anyhow, bail, ensure};
use confirmation::UiPrompt;
use load_records::{list_tasks, load, task_status};
use pwf_client::{PwfClient, pb};
use pwf_models::{
    project::ProjectId,
    revision::ContentRevision,
    task::{TaskId, TaskStatus},
};
use task_mutation::submit;
use tokio::sync::{mpsc, oneshot};

use crate::{
    browser::{Project, ProjectScope, Snapshot},
    draft::{Draft, Mutation, TaskAction, TaskTarget},
    references,
};

pub(super) const OPERATION_TIMEOUT: Duration = Duration::from_secs(30);

pub(super) enum Work {
    Load {
        scope: ProjectScope,
    },
    NewTask(ProjectId),
    Action {
        id: TaskId,
        action: TaskAction,
    },
    Blockers {
        exclude: TaskId,
    },
    Submit(Mutation),
    Inspect {
        project: Option<ProjectId>,
        task: Option<TaskId>,
    },
}

impl Work {
    pub fn writes(&self) -> bool {
        matches!(
            self,
            Self::Submit(_)
                | Self::Action {
                    action: TaskAction::Complete
                        | TaskAction::Backlog
                        | TaskAction::Activate
                        | TaskAction::Delete,
                    ..
                }
        )
    }
}

pub(super) struct Blocker {
    pub id: TaskId,
    pub title: String,
    pub status: TaskStatus,
}

pub(super) struct Question {
    pub title: String,
    pub lines: Vec<String>,
}

pub(super) enum Outcome {
    Loaded(Snapshot),
    Draft(Draft),
    Blockers(Vec<Blocker>),
    Saved {
        message: String,
    },
    Exported(PathBuf),
    Aborted,
    EditFile(PathBuf),
    Inspected {
        snapshot: Snapshot,
        revision: Option<ContentRevision>,
        current: String,
    },
}

pub(super) enum WorkerEvent {
    Projects {
        id: u64,
        projects: Vec<Project>,
    },
    Finished {
        id: u64,
        result: Result<Outcome, String>,
    },
    Confirm {
        id: u64,
        question: Question,
        reply: oneshot::Sender<bool>,
    },
}

pub(super) async fn run(work: Work, id: u64, events: mpsc::Sender<WorkerEvent>) {
    let result = tokio::time::timeout(OPERATION_TIMEOUT, perform_work(work, id, events.clone()))
        .await
        .map_err(|_| {
            anyhow!("The operation timed out. Inspect saved state before retrying a write.")
        })
        .and_then(std::convert::identity)
        .map_err(|error| describe_error(&error));
    let _ = events.send(WorkerEvent::Finished { id, result }).await;
}

fn describe_error(error: &anyhow::Error) -> String {
    if let Some(status) = error
        .chain()
        .find_map(|cause| cause.downcast_ref::<tonic::Status>())
    {
        return match status.code() {
            tonic::Code::Aborted => "The task changed after it was read.".into(),
            _ => status.message().to_string(),
        };
    }
    format!("{error:#}")
}

async fn perform_work(work: Work, id: u64, events: mpsc::Sender<WorkerEvent>) -> Result<Outcome> {
    if let Work::Submit(Mutation::Export { path, references }) = work {
        return tokio::task::spawn_blocking(move || {
            references::export(&path, &references)?;
            Ok(Outcome::Exported(path))
        })
        .await?;
    }
    let client = PwfClient::connect_local()
        .await
        .context("Cannot connect to pwf-server. Run `pwf doctor`; press r to reconnect.")?;
    let prompt = UiPrompt {
        events: events.clone(),
        id,
    };
    match work {
        Work::Load { scope } => Ok(Outcome::Loaded(load(&client, scope, id, &events).await?)),
        Work::NewTask(project) => Ok(Outcome::Draft(task_template(&client, project).await?)),
        Work::Action { id, action } => task_action(&client, id, action, prompt).await,
        Work::Blockers { exclude } => {
            let tasks = list_tasks(&client, None, false, Ok).await?;
            let mut blockers = Vec::new();
            for task in tasks {
                let id = TaskId::try_new(task.id)?;
                if id != exclude {
                    blockers.push(Blocker {
                        id,
                        title: task.heading,
                        status: task_status(task.status)?,
                    });
                }
            }
            Ok(Outcome::Blockers(blockers))
        }
        Work::Submit(mutation) => submit(&client, mutation, prompt).await,
        Work::Inspect { project, task } => {
            inspection::inspect(&client, project, task, id, &events).await
        }
    }
}

async fn task_template(client: &PwfClient, project: ProjectId) -> Result<Draft> {
    let response = client
        .task()
        .get_task_body_sections(pb::GetTaskBodySectionsRequest {
            project_id: Some(project.to_string()),
        })
        .await?;
    ensure!(
        !response.sections.is_empty(),
        "The server returned an empty task template."
    );
    let mut template = String::new();
    for section in response.sections {
        ensure!(
            (1..=6).contains(&section.heading_level),
            "The server returned an invalid heading level."
        );
        write!(
            template,
            "{} {}\n\n",
            "#".repeat(section.heading_level as usize),
            section.header
        )?;
    }
    Ok(Draft::new_task(project, response.preset, &template))
}

async fn task_action(
    client: &PwfClient,
    id: TaskId,
    action: TaskAction,
    prompt: UiPrompt,
) -> Result<Outcome> {
    if action == TaskAction::EditFile {
        let record = client
            .task()
            .get_task_record(pb::GetTaskRecordRequest { id: id.to_string() })
            .await?
            .record
            .ok_or_else(|| anyhow!("The server returned no task record."))?;
        ensure!(
            !record.locator.is_empty(),
            "The server returned no task file path."
        );
        return Ok(Outcome::EditFile(PathBuf::from(record.locator)));
    }
    match action {
        TaskAction::Activate => return submit(client, Mutation::Activate(id), prompt).await,
        TaskAction::Delete => return submit(client, Mutation::Delete(id), prompt).await,
        TaskAction::Backlog => return submit(client, Mutation::Backlog(id), prompt).await,
        _ => {}
    }
    let target = TaskTarget::from(
        client
            .task()
            .get_task(pb::GetTaskRequest { id: id.to_string() })
            .await?,
    );
    match action {
        TaskAction::Metadata => Ok(Outcome::Draft(Draft::metadata(target))),
        TaskAction::CompleteReport => Ok(Outcome::Draft(Draft::report(target, false))),
        TaskAction::Cancel => Ok(Outcome::Draft(Draft::report(target, true))),
        TaskAction::Complete => {
            submit(
                client,
                Mutation::Complete {
                    target,
                    report: None,
                    commits: Vec::new(),
                },
                prompt,
            )
            .await
        }
        TaskAction::EditFile | TaskAction::Backlog | TaskAction::Activate | TaskAction::Delete => {
            bail!("Unsupported task action.")
        }
    }
}
