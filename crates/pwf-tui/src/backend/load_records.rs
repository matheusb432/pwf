use std::{io::Read as _, path::PathBuf};

use anyhow::{Context as _, Result, bail, ensure};
use pwf_client::{PwfClient, pb};
use pwf_models::{
    note::NoteId,
    project::ProjectId,
    task::{TaskId, TaskStatus},
};

use super::WorkerEvent;
use crate::browser::{Project, Record, RecordId, Snapshot};

const RECORDS_MAX: usize = 10_000;
const SNAPSHOT_BYTES_MAX: usize = 64 * 1024 * 1024;
const FILE_BYTES_MAX: u64 = 4 * 1024 * 1024;
const PAGE_SIZE: u32 = 256;

pub(super) async fn load(
    client: &PwfClient,
    scope: Option<ProjectId>,
    id: u64,
    events: &tokio::sync::mpsc::Sender<WorkerEvent>,
) -> Result<Snapshot> {
    let projects = client
        .project()
        .list_projects(pb::ListProjectsRequest {
            status: pb::ProjectStatusFilter::ActiveOnly as i32,
        })
        .await?
        .projects;
    ensure!(
        projects.len() <= 1024,
        "Project listing exceeds 1024 projects."
    );
    let mut projects = projects
        .into_iter()
        .map(|project| {
            Ok(Project {
                id: ProjectId::try_new(project.id)?,
                title: project.title,
                tasks_path: expand_home(&project.tasks_path)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    projects.sort_by(|left, right| left.id.cmp(&right.id));
    let _ = events
        .send(WorkerEvent::Projects {
            id,
            projects: projects.clone(),
        })
        .await;
    if let Some(id) = &scope {
        ensure!(
            projects.iter().any(|project| &project.id == id),
            "No active project {id}."
        );
    }
    let tasks = list_tasks(client, scope.as_ref(), true).await?;
    let mut records = Vec::new();
    let mut bytes = 0;
    for task in tasks {
        let record = task_record(task)?;
        bytes += record.bytes();
        ensure!(
            bytes <= SNAPSHOT_BYTES_MAX,
            "Saved content exceeds 64 MiB; select a project before refreshing."
        );
        records.push(record);
    }
    let mut notes = Vec::new();
    for project in &projects {
        if scope.as_ref().is_some_and(|id| &project.id != id) {
            continue;
        }
        let listed = client
            .note()
            .list_notes(pb::ListNotesRequest {
                project_id: project.id.to_string(),
                limit_kind: pb::NoteListLimitKind::AtMost as i32,
                limit: RECORDS_MAX as u64,
            })
            .await?;
        ensure!(
            listed.hidden == 0 && records.len() + notes.len() + listed.notes.len() <= RECORDS_MAX,
            "Record listing exceeds 10000 records; select a project before refreshing."
        );
        for note in listed.notes {
            let id = NoteId::try_new(note.id)?;
            ensure!(
                id.project_id() == &project.id,
                "The server returned a note from another project."
            );
            let path = project.tasks_path.join(format!("{id}.md"));
            let mut record = Record::new(RecordId::Note(id), project.id.clone(), note.title, path);
            record.verified = note.is_verified;
            record
                .metadata
                .push(("Project".into(), project.title.clone()));
            notes.push(record);
        }
    }
    let (notes, warnings) = tokio::task::spawn_blocking(move || read_notes(notes, bytes)).await??;
    records.extend(notes);
    records.sort_by(|left, right| right.id.as_str().cmp(left.id.as_str()));
    Ok(Snapshot {
        projects,
        records,
        warnings,
    })
}

pub(super) async fn list_tasks(
    client: &PwfClient,
    project: Option<&ProjectId>,
    detailed: bool,
) -> Result<Vec<pb::ListedTask>> {
    let mut tasks = Vec::new();
    let mut bytes = 0;
    let mut page_token = None;
    for _ in 0..RECORDS_MAX.div_ceil(PAGE_SIZE as usize) {
        let response = client
            .task()
            .list_tasks(pb::ListTasksRequest {
                project_id: project.map(ToString::to_string),
                all: true,
                status: Some(pb::TaskStatusFilter::All as i32),
                detail: if detailed {
                    pb::ListDetail::Detailed
                } else {
                    pb::ListDetail::Summary
                } as i32,
                page_size: PAGE_SIZE,
                page_token,
                ..Default::default()
            })
            .await?;
        for task in response.tasks {
            bytes += task.body.len()
                + task.source.as_ref().map_or(0, String::len)
                + task.heading.len()
                + task.raw_tags.as_ref().map_or(0, String::len);
            ensure!(
                bytes <= SNAPSHOT_BYTES_MAX,
                "Task data exceeds 64 MiB; select a project before refreshing."
            );
            tasks.push(task);
        }
        ensure!(
            tasks.len() <= RECORDS_MAX,
            "Task listing exceeds 10000 tasks; select a project before refreshing."
        );
        page_token = response.next_page_token;
        if page_token.is_none() {
            return Ok(tasks);
        }
    }
    bail!("Task listing exceeds its page limit; select a project before refreshing.")
}

fn read_notes(mut notes: Vec<Record>, mut bytes: usize) -> Result<(Vec<Record>, Vec<String>)> {
    let mut warnings = Vec::new();
    for record in &mut notes {
        match read_note(&record.path) {
            Ok(body) => record.body = body,
            Err(error) => {
                let diagnostic = format!("Cannot preview {}: {error:#}", record.path.display());
                warnings.push(diagnostic.clone());
                record.diagnostic = Some(diagnostic);
            }
        }
        record.index();
        bytes += record.bytes();
        ensure!(
            bytes <= SNAPSHOT_BYTES_MAX,
            "Saved content exceeds 64 MiB; select a project before refreshing."
        );
    }
    Ok((notes, warnings))
}

fn read_note(path: &std::path::Path) -> Result<String> {
    let mut source = String::new();
    std::fs::File::open(path)?
        .take(FILE_BYTES_MAX + 1)
        .read_to_string(&mut source)?;
    ensure!(source.len() as u64 <= FILE_BYTES_MAX, "note exceeds 4 MiB");
    let mut lines = source.split_inclusive('\n');
    let first = lines.next().unwrap_or_default();
    if first.trim_start_matches('\u{feff}').trim() != "---" {
        return Ok(source);
    }
    let mut offset = first.len();
    for line in lines {
        offset += line.len();
        if line.trim() == "---" {
            return Ok(source[offset..].to_string());
        }
    }
    bail!("unclosed Markdown frontmatter")
}

fn expand_home(raw: &str) -> Result<PathBuf> {
    if let Some(path) = raw.strip_prefix("~/").or_else(|| raw.strip_prefix("~\\")) {
        return Ok(std::env::home_dir()
            .context("Cannot resolve the home directory.")?
            .join(path));
    }
    Ok(PathBuf::from(raw))
}

pub(super) fn task_status(raw: i32) -> Result<TaskStatus> {
    Ok(match pb::TaskStatus::try_from(raw)? {
        pb::TaskStatus::Active => TaskStatus::Active,
        pb::TaskStatus::Done => TaskStatus::Done,
        pb::TaskStatus::Cancelled => TaskStatus::Cancelled,
        pb::TaskStatus::Backlog => TaskStatus::Backlog,
        pb::TaskStatus::Unspecified => bail!("The server returned an unspecified task status."),
    })
}

fn task_record(task: pb::ListedTask) -> Result<Record> {
    let id = TaskId::try_new(task.id)?;
    let mut record = Record::new(
        RecordId::Task(id.clone()),
        id.project_id().clone(),
        task.heading,
        task.file_path.into(),
    );
    record.status = Some(task_status(task.status)?);
    record.body = task.body;
    let mut metadata = vec![("Project".to_string(), task.project)];
    for (label, value) in [
        ("Tags", task.raw_tags),
        ("Created", task.created_at),
        ("Completed", task.completed_at),
        ("Commits", task.commits),
    ] {
        if let Some(value) = value {
            metadata.push((label.to_string(), value));
        }
    }
    if let Some(priority) = task.priority {
        metadata.push((
            "Priority".into(),
            pb::PriorityTier::try_from(priority)
                .context("invalid priority")?
                .as_str_name()
                .trim_start_matches("PRIORITY_TIER_")
                .to_lowercase(),
        ));
    }
    if let Some(effort) = task.effort {
        metadata.push((
            "Effort".into(),
            pb::EffortTier::try_from(effort)
                .context("invalid effort")?
                .as_str_name()
                .trim_start_matches("EFFORT_TIER_")
                .to_lowercase(),
        ));
    }
    if !task.blocked_by.is_empty() {
        metadata.push(("Blocked by".into(), task.blocked_by.join(", ")));
    }
    record.metadata = metadata;
    record.index();
    Ok(record)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn note_previews_exclude_frontmatter_and_report_read_errors() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("PWF-NOTE-0001.md");
        std::fs::write(
            &path,
            "\u{feff}---\r\nid: PWF-NOTE-0001\r\n---\r\n# Note\r\nbody",
        )
        .unwrap();
        assert_eq!(read_note(&path).unwrap(), "# Note\r\nbody");
        std::fs::write(&path, "---\nid: broken").unwrap();
        assert!(read_note(&path).is_err());
    }
}
