use std::{
    io::Read as _,
    path::{Path, PathBuf},
};

use anyhow::{Context as _, Result, bail, ensure};
use pwf_client::{PwfClient, pb};
use pwf_models::{
    note::NoteId,
    project::ProjectId,
    task::{TaskId, TaskStatus},
};

use super::WorkerEvent;
use crate::browser::{Project, ProjectScope, Record, RecordId, Snapshot};

const RECORDS_MAX: usize = 10_000;
const SNAPSHOT_BYTES_MAX: usize = 64 * 1024 * 1024;
const FILE_BYTES_MAX: u64 = 4 * 1024 * 1024;
const PAGE_SIZE: u32 = 256;

pub(super) async fn load(
    client: &PwfClient,
    scope: ProjectScope,
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
    let (mut projects, scope) = tokio::task::spawn_blocking(move || {
        let scope = match scope {
            ProjectScope::All => None,
            ProjectScope::Project(id) => Some(id),
            ProjectScope::Directory(directory) => infer_project(&projects, &directory)?,
        };
        let projects = projects
            .into_iter()
            .map(|project| {
                Ok(Project {
                    id: ProjectId::try_new(project.id)?,
                    title: project.title,
                    tasks_path: expand_home(&project.tasks_path)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok::<_, anyhow::Error>((projects, scope))
    })
    .await??;
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
    let mut records = list_tasks(client, scope.as_ref(), true, task_record).await?;
    let mut bytes = 0;
    for record in &records {
        bytes += record.bytes();
        ensure!(
            bytes <= SNAPSHOT_BYTES_MAX,
            "Saved content exceeds 64 MiB; select a project before refreshing."
        );
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
        project: scope,
        projects,
        records,
        warnings,
    })
}

fn infer_project(projects: &[pb::Project], directory: &Path) -> Result<Option<ProjectId>> {
    let directory =
        std::fs::canonicalize(directory).context("Cannot resolve the working directory.")?;
    let mut closest = None;
    let mut depth = 0;
    for project in projects {
        let source = project
            .source_value
            .as_deref()
            .filter(|_| project.source_kind.as_deref() == Some("directory"));
        for raw in [Some(project.tasks_path.as_str()), source]
            .into_iter()
            .flatten()
        {
            let root = expand_home(raw)?;
            let root = std::fs::canonicalize(&root).unwrap_or(root);
            let root_depth = root.components().count();
            if directory.starts_with(&root) && root_depth > depth {
                closest = Some(ProjectId::try_new(project.id.clone())?);
                depth = root_depth;
            }
        }
    }
    Ok(closest)
}

pub(super) async fn list_tasks<T>(
    client: &PwfClient,
    project: Option<&ProjectId>,
    detailed: bool,
    mut convert: impl FnMut(pb::ListedTask) -> Result<T>,
) -> Result<Vec<T>> {
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
                    pb::ListDetail::Preview
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
            tasks.push(convert(task)?);
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
            Ok(body) => record.set_body(body),
            Err(error) => {
                let diagnostic = format!("Cannot preview {}: {error:#}", record.path.display());
                warnings.push(diagnostic.clone());
                record.diagnostic = Some(diagnostic);
            }
        }
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
    if raw == "~" {
        return std::env::home_dir().context("Cannot resolve the home directory.");
    }
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
    record.set_body(task.body);
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
    Ok(record)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_inference_uses_directory_boundaries_and_the_deepest_root() {
        let root = tempfile::tempdir().unwrap();
        for path in [
            "source/child/src",
            "source-other",
            "tasks/child",
            "tasks-other",
        ] {
            std::fs::create_dir_all(root.path().join(path)).unwrap();
        }
        let project = |id: &str, tasks: &str, source: &str| pb::Project {
            id: id.into(),
            tasks_path: root.path().join(tasks).to_str().unwrap().into(),
            source_kind: Some("directory".into()),
            source_value: Some(root.path().join(source).to_str().unwrap().into()),
            ..Default::default()
        };
        let projects = [
            project("ROOT", "tasks", "source"),
            project("LEAF", "tasks/child", "source/child"),
        ];
        for (directory, expected) in [
            ("source", Some("ROOT")),
            ("source/child/src", Some("LEAF")),
            ("tasks/child", Some("LEAF")),
            ("source-other", None),
            ("tasks-other", None),
        ] {
            assert_eq!(
                infer_project(&projects, &root.path().join(directory))
                    .unwrap()
                    .map(|id| id.to_string())
                    .as_deref(),
                expected,
            );
        }
        #[cfg(unix)]
        {
            let alias = root.path().join("linked-source");
            std::os::unix::fs::symlink(root.path().join("source/child"), &alias).unwrap();
            assert_eq!(
                infer_project(&projects, &alias.join("src"))
                    .unwrap()
                    .unwrap()
                    .as_ref(),
                "LEAF",
            );
        }
    }

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
