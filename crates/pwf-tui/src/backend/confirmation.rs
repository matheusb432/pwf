use std::time::Duration;

use pwf_client::confirmation::{Confirmation, ConfirmationPrompt};
use tokio::sync::{mpsc, oneshot};

use super::{Question, WorkerEvent};

pub(super) struct UiPrompt {
    pub events: mpsc::Sender<WorkerEvent>,
    pub id: u64,
}

#[derive(Debug, thiserror::Error)]
#[error("The confirmation expired or the terminal closed; no approval was sent.")]
pub(super) struct PromptClosed;

impl ConfirmationPrompt for UiPrompt {
    type Error = PromptClosed;

    async fn confirm(&self, confirmation: &Confirmation) -> Result<bool, Self::Error> {
        let question = match confirmation {
            Confirmation::DeleteTask(value) => Question {
                title: format!("Delete {}?", value.task_id),
                lines: vec![
                    value.title.clone(),
                    format!("Project: {}", value.project),
                    format!("File: {}", value.file_path),
                    value.trash_folder.as_ref().map_or_else(
                        || "Deletion: hard delete".to_string(),
                        |trash| format!("Trash: {trash}"),
                    ),
                    value
                        .obsidian_vault
                        .as_ref()
                        .map_or_else(String::new, |vault| format!("Vault: {vault}")),
                ],
            },
            Confirmation::ActivateTask(value) => Question {
                title: format!("Reopen {}?", value.task_id),
                lines: vec![
                    "Reopening removes completion metadata and the appended report.".into(),
                    format!(
                        "Completed: {}",
                        value.completion_date.as_deref().unwrap_or("-")
                    ),
                    format!("Commits: {}", value.commits.as_deref().unwrap_or("-")),
                    format!("Report: {}", value.report.as_deref().unwrap_or("-")),
                ],
            },
            Confirmation::DeleteNote(_) | Confirmation::DispatchSession(_) => {
                return Err(PromptClosed);
            }
        };
        let (reply, decision) = oneshot::channel();
        self.events
            .send(WorkerEvent::Confirm {
                id: self.id,
                question,
                reply,
            })
            .await
            .map_err(|_| PromptClosed)?;
        tokio::time::timeout(Duration::from_secs(25), decision)
            .await
            .map_err(|_| PromptClosed)?
            .map_err(|_| PromptClosed)
    }
}
