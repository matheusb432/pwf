use anyhow::{Context as _, Result, bail};
use pwf_client::{PwfClient, pb};
use pwf_models::task::TaskId;

use super::{Outcome, confirmation::UiPrompt};
use crate::{
    draft::{Mutation, TaskEdit},
    draft_file,
};

fn task_update(edit: TaskEdit) -> pb::UpdateTaskRequest {
    let original = &edit.target;
    pb::UpdateTaskRequest {
        id: original.id.to_string(),
        expected_revision: Some(original.revision.to_string()),
        content: (edit.title != original.title).then(|| pb::TaskContentEdit {
            content: Some(pb::task_content_edit::Content::Title(
                edit.title.to_string(),
            )),
        }),
        tags: (edit.tags != original.tags).then(|| {
            collection_edit(
                edit.tags
                    .map(|tags| tags.iter().map(ToString::to_string).collect())
                    .unwrap_or_default(),
            )
        }),
        blocked_by: (edit.blocked_by != original.blocked_by).then(|| {
            collection_edit(
                edit.blocked_by
                    .map(|ids| ids.iter().map(ToString::to_string).collect())
                    .unwrap_or_default(),
            )
        }),
        priority: (edit.priority != original.priority).then(|| pb::PriorityEdit {
            operation: Some(edit.priority.map_or_else(
                || pb::priority_edit::Operation::Clear(pb::ClearField {}),
                |tier| {
                    pb::priority_edit::Operation::Set(match tier {
                        pwf_models::task::PriorityTier::Low => pb::PriorityTier::Low,
                        pwf_models::task::PriorityTier::Medium => pb::PriorityTier::Medium,
                        pwf_models::task::PriorityTier::High => pb::PriorityTier::High,
                        pwf_models::task::PriorityTier::Highest => pb::PriorityTier::Highest,
                    } as i32)
                },
            )),
        }),
        effort: (edit.effort != original.effort).then(|| pb::EffortEdit {
            operation: Some(edit.effort.map_or_else(
                || pb::effort_edit::Operation::Clear(pb::ClearField {}),
                |tier| {
                    pb::effort_edit::Operation::Set(match tier {
                        pwf_models::task::EffortTier::Low => pb::EffortTier::Low,
                        pwf_models::task::EffortTier::Medium => pb::EffortTier::Medium,
                        pwf_models::task::EffortTier::High => pb::EffortTier::High,
                        pwf_models::task::EffortTier::Highest => pb::EffortTier::Highest,
                    } as i32)
                },
            )),
        }),
    }
}

fn collection_edit(values: Vec<String>) -> pb::StringCollectionEdit {
    pb::StringCollectionEdit {
        operation: Some(if values.is_empty() {
            pb::string_collection_edit::Operation::Clear(pb::ClearField {})
        } else {
            pb::string_collection_edit::Operation::Replace(pb::StringValues { values })
        }),
    }
}

pub(super) async fn submit(
    client: &PwfClient,
    mutation: Mutation,
    prompt: UiPrompt,
) -> Result<Outcome> {
    let tasks = client.task();
    let (verb, summary) = match mutation {
        Mutation::CreateTask {
            project,
            title,
            body,
        } => {
            let response = create_task(client, project, title, body).await?;
            ("Created", response.task)
        }
        Mutation::CreateNote {
            project,
            title,
            body,
        } => return create_note(client, project, title, body).await,
        Mutation::UpdateTask(edit) => ("Updated", tasks.update_task(task_update(edit)).await?.task),
        Mutation::Complete {
            target,
            report,
            commits,
        } => (
            "Completed",
            tasks
                .complete_task(pb::CompleteTaskRequest {
                    id: target.id.to_string(),
                    expected_revision: Some(target.revision.to_string()),
                    report: report.map(|report| report.to_string()),
                    commits,
                })
                .await?
                .task,
        ),
        Mutation::Cancel {
            target,
            report,
            commits,
        } => (
            "Cancelled",
            tasks
                .cancel_task(pb::CancelTaskRequest {
                    id: target.id.to_string(),
                    expected_revision: Some(target.revision.to_string()),
                    report: report.to_string(),
                    commits,
                })
                .await?
                .task,
        ),
        Mutation::Backlog(id) => {
            let response = tasks
                .backlog_task(pb::BacklogTaskRequest { id: id.to_string() })
                .await?;
            match response.outcome {
                Some(pb::backlog_task_response::Outcome::Backlogged(result)) => {
                    ("Backlogged", result.task)
                }
                Some(pb::backlog_task_response::Outcome::AlreadyBacklogged(result)) => {
                    ("Already backlogged", result.task)
                }
                None => bail!("The server returned no backlog outcome."),
            }
        }
        Mutation::Activate(id) => {
            let response = tasks
                .activate_task(pb::ActivateTaskStart { id: id.to_string() }, prompt)
                .await?;
            match response.outcome {
                Some(pb::activate_task_result::Outcome::Activated(result)) => {
                    ("Activated", result.task)
                }
                Some(pb::activate_task_result::Outcome::AlreadyActive(result)) => {
                    ("Already active", result.task)
                }
                Some(pb::activate_task_result::Outcome::Aborted(_)) => return Ok(Outcome::Aborted),
                None => bail!("The server returned no activation outcome."),
            }
        }
        Mutation::Delete(id) => {
            let response = tasks
                .delete_task(pb::DeleteTaskStart { id: id.to_string() }, prompt)
                .await?;
            match response.outcome {
                Some(pb::delete_task_result::Outcome::Deleted(result)) => ("Deleted", result.task),
                Some(pb::delete_task_result::Outcome::Aborted(_)) => return Ok(Outcome::Aborted),
                None => bail!("The server returned no deletion outcome."),
            }
        }
        Mutation::Export { .. } => bail!("Reference exports do not use the server."),
    };
    let summary = summary
        .context("The server returned no task summary. Inspect saved state before retrying.")?;
    Ok(Outcome::Saved {
        message: format!(
            "{verb} {} · {}",
            TaskId::try_new(summary.id)?,
            summary.title
        ),
    })
}

async fn create_task(
    client: &PwfClient,
    project: pwf_models::project::ProjectId,
    title: pwf_models::task::TaskTitle,
    body: String,
) -> Result<pb::CreateTaskFromFileResponse> {
    let source = tokio::task::spawn_blocking(move || draft_file::create(&body)).await??;
    Ok(client
        .task()
        .create_task_from_file(pb::CreateTaskFromFileRequest {
            project_id: project.to_string(),
            title: Some(title.to_string()),
            source_file: source
                .path()
                .to_str()
                .context("Draft path is not UTF-8.")?
                .to_string(),
        })
        .await?)
}

async fn create_note(
    client: &PwfClient,
    project: pwf_models::project::ProjectId,
    title: pwf_models::note::NoteTitle,
    body: String,
) -> Result<Outcome> {
    let response = client
        .note()
        .add_note(pb::AddNoteRequest {
            project_id: project.to_string(),
            title: title.to_string(),
            content: body,
            ..Default::default()
        })
        .await?;
    let id = pwf_models::note::NoteId::try_new(response.id)?;
    Ok(Outcome::Saved {
        message: format!("Created note {id} · {}", response.title),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::task_target;
    #[test]
    fn metadata_updates_omit_unchanged_fields_and_send_explicit_clears() {
        let mut target = task_target();
        target.effort = None;
        target.blocked_by = None;
        let request = task_update(TaskEdit {
            title: target.title.clone(),
            tags: None,
            priority: None,
            effort: Some(pwf_models::task::EffortTier::Highest),
            blocked_by: None,
            target,
        });
        assert!(request.content.is_none());
        assert!(request.blocked_by.is_none());
        assert!(matches!(
            request.tags.unwrap().operation,
            Some(pb::string_collection_edit::Operation::Clear(_))
        ));
        assert!(matches!(
            request.priority.unwrap().operation,
            Some(pb::priority_edit::Operation::Clear(_))
        ));
        assert!(
            matches!(request.effort.unwrap().operation, Some(pb::effort_edit::Operation::Set(value)) if value == pb::EffortTier::Highest as i32)
        );
        assert_eq!(
            request.expected_revision.as_deref(),
            Some("a".repeat(64).as_str())
        );
    }
}
