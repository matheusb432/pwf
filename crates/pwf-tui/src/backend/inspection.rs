use std::fmt::Write as _;

use anyhow::Result;
use pwf_client::{PwfClient, pb};
use pwf_models::{project::ProjectId, revision::ContentRevision, task::TaskId};
use tokio::sync::mpsc;

use super::{Outcome, WorkerEvent, load_records::load};

pub(super) async fn inspect(
    client: &PwfClient,
    project: Option<ProjectId>,
    task: Option<TaskId>,
    request_id: u64,
    events: &mpsc::Sender<WorkerEvent>,
) -> Result<Outcome> {
    let snapshot = load(client, project.into(), request_id, events).await?;
    let (revision, current) = match task {
        Some(id) => inspect_task(client, &id)
            .await
            .map(|(revision, current)| (Some(revision), current))?,
        None => (None, inspect_listing(&snapshot)?),
    };
    Ok(Outcome::Inspected {
        snapshot,
        revision,
        current,
    })
}

async fn inspect_task(client: &PwfClient, id: &TaskId) -> Result<(ContentRevision, String)> {
    let task = client
        .task()
        .get_task(pb::GetTaskRequest { id: id.to_string() })
        .await?;
    let tags = task.tags.as_ref().map_or_else(
        || "none".into(),
        |tags| {
            tags.iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        },
    );
    let blockers = task.blocked_by.as_ref().map_or_else(
        || "none".into(),
        |ids| {
            ids.iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        },
    );
    let current = format!(
        "Saved {} is {}: {}\n\nTags: {tags}\nPriority: {}\nEffort: {}\nBlocked by: {blockers}\nCommits: {}\n\n{}",
        task.id,
        task.status,
        task.title,
        task.priority
            .map_or_else(|| "none".into(), |tier| tier.to_string()),
        task.effort
            .map_or_else(|| "none".into(), |tier| tier.to_string()),
        task.commits
            .as_ref()
            .map_or("none", |commits| commits.as_ref()),
        task.body,
    );
    Ok((task.revision, current))
}

fn inspect_listing(snapshot: &crate::browser::Snapshot) -> Result<String> {
    let mut current = "Saved state refreshed. Check for an existing task or note before creating again; the previous request may have succeeded.\n\n".to_string();
    for record in &snapshot.records {
        writeln!(
            current,
            "{} [{}] {}",
            record.id.as_str(),
            record.status_label(),
            record.title
        )?;
    }
    Ok(current)
}
