use pwf_models::{
    project::Project,
    task::{EffortTier, Prerequisites, Tags, TaskId, TaskStatus, TaskTitle},
};

use super::{
    normalize_commit_ranges,
    note_body::{append_lanes, append_report_block, render},
    prerequisites::{self, PrerequisiteValidationError, validate_and_merge},
    resolve_task_project::{self, ResolveTaskProject, ResolveTaskProjectError},
    tags, task_body_region,
};
use crate::{
    ports::task_record::{NullablePatch, TaskPatch, TaskRecord, TaskStore},
    project::{
        get_active_project::{self, GetActiveProject},
        get_project::GetProjectError,
    },
};

struct TaskIdentity {
    project: Project,
    identifier: TaskId,
}

struct PreparedTaskUpdate {
    identity: TaskIdentity,
    patch: TaskPatch,
    outcome: UpdateTaskOk,
}

/// Requests edits to one task.
#[derive(Debug, Clone)]
pub struct UpdateTask {
    /// Requested task identifier.
    pub id: TaskId,
    /// Optional replacement prompt.
    pub prompt: Option<String>,
    /// Optional replacement title.
    pub title: Option<TaskTitle>,
    /// Optional lane content appended to the body.
    pub append: Option<String>,
    /// Prerequisite task IDs appended to existing prerequisites.
    pub prereq: Option<Prerequisites>,
    /// Whether existing prerequisites are cleared.
    pub clear_prereq: bool,
    /// Raw repeated commit ranges.
    pub commits: Vec<String>,
    /// Optional report appended to the body.
    pub append_report: Option<String>,
    /// Optional replacement effort tier.
    pub effort: Option<EffortTier>,
    /// Raw tags appended to existing tags.
    pub tags: Vec<String>,
    /// Whether existing tags are cleared before applying raw tags.
    pub tags_clear: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum UpdateTaskError {
    #[error("Task not found: {id}")]
    TaskNotFound { id: TaskId },
    #[error(
        "nothing to update (pass --prompt, --title, --prereq, --clear-prereq, --tag, --tags-clear, --commits, --append-report, --append, and/or --effort)."
    )]
    NothingToUpdate,
    #[error(
        "only --commits / --append-report can amend closed task {id} (done/cancelled); body/title/prereq/tags/append/effort need an active task."
    )]
    ClosedTaskAmendOnly { id: TaskId },
    #[error("task {id} has invalid tags frontmatter: {raw:?}.")]
    InvalidTagsFrontmatter { id: TaskId, raw: String },
    #[error(
        "Invalid --tag value {raw:?}; use lowercase/uppercase ASCII letters, digits, '_' or '-', without leading, trailing, or repeated separators."
    )]
    InvalidTag { raw: String },
    #[error("Unknown --prereq id(s): {}.", format_task_ids(ids))]
    UnknownPrereqIds { ids: Vec<TaskId> },
    #[error("--report cannot be empty.")]
    EmptyReport,
    #[error("--append cannot be empty.")]
    EmptyAppend,
    #[error("{0}")]
    WriteStore(#[source] Box<dyn std::error::Error + Send + Sync>),
    #[error("{0}")]
    QueryProject(#[source] Box<dyn std::error::Error + Send + Sync>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateTaskOk {
    OpenTaskEdit {
        id: TaskId,
        project: String,
        title: String,
    },
    Changed {
        id: TaskId,
        changes: Vec<String>,
    },
}

/// Applies body and frontmatter edits in one task patch.
///
/// Closed tasks accept only commit and report amendments.
///
/// # Errors
///
/// Returns [`UpdateTaskError`] when preparation, validation, or persistence fails.
#[cqrsy::command]
pub async fn execute(
    command: UpdateTask,
    store: &impl TaskStore,
    pool: &sqlx::SqlitePool,
) -> Result<UpdateTaskOk, UpdateTaskError> {
    let task_project = resolve_task_project::execute(
        ResolveTaskProject {
            id: command.id.clone(),
        },
        pool,
    )
    .await
    .map_err(|error| match error {
        ResolveTaskProjectError::UnknownProjectId { .. } => UpdateTaskError::TaskNotFound {
            id: command.id.clone(),
        },
        ResolveTaskProjectError::QueryProject(source) => UpdateTaskError::QueryProject(source),
    })?;
    let mut projects = vec![task_project];
    if let Some(prerequisites) = command.prereq.as_ref() {
        let ids = prerequisites::project_ids(prerequisites);
        for id in ids {
            if projects.iter().any(|project| project.id == id) {
                continue;
            }
            match get_active_project::execute(GetActiveProject { id }, pool).await {
                Ok(project) => projects.push(project),
                Err(GetProjectError::ProjectNotFound { .. }) => {}
                Err(error) => return Err(UpdateTaskError::QueryProject(Box::new(error))),
            }
        }
    }
    let project = &projects[0];
    let prepared = prepare(&command, store, project, &projects)?;
    persist(prepared, store)
}

fn format_task_ids(ids: &[TaskId]) -> String {
    ids.iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

fn prepare(
    command: &UpdateTask,
    store: &impl TaskStore,
    project: &Project,
    projects: &[Project],
) -> Result<PreparedTaskUpdate, UpdateTaskError> {
    let commits = normalize_commit_ranges(&command.commits);
    let tags = parse_tags(&command.tags)?;
    let edits_body = command.prompt.is_some()
        || command.title.is_some()
        || command.prereq.is_some()
        || command.clear_prereq
        || command.append.is_some()
        || command.effort.is_some()
        || tags.is_some()
        || command.tags_clear;
    if !edits_body && commits.is_none() && command.append_report.is_none() {
        return Err(UpdateTaskError::NothingToUpdate);
    }

    let not_found = || UpdateTaskError::TaskNotFound {
        id: command.id.clone(),
    };
    let record = store
        .get(project, &command.id)
        .map_err(|error| UpdateTaskError::WriteStore(Box::new(error)))?
        .ok_or_else(not_found)?;
    let identity = TaskIdentity {
        project: project.clone(),
        identifier: command.id.clone(),
    };

    if record.status == TaskStatus::Active {
        prepare_open_task(
            store,
            projects,
            identity,
            command,
            commits.as_deref(),
            tags.as_ref(),
            &record,
        )
    } else if edits_body {
        Err(UpdateTaskError::ClosedTaskAmendOnly {
            id: command.id.clone(),
        })
    } else {
        amend_closed_task(identity, command, commits.as_deref(), &record)
    }
}

fn persist(
    prepared: PreparedTaskUpdate,
    store: &impl TaskStore,
) -> Result<UpdateTaskOk, UpdateTaskError> {
    store
        .update(
            &prepared.identity.project,
            &prepared.identity.identifier,
            prepared.patch,
        )
        .map_err(|error| UpdateTaskError::WriteStore(Box::new(error)))?;
    Ok(prepared.outcome)
}

fn prepare_open_task(
    store: &impl TaskStore,
    projects: &[Project],
    identity: TaskIdentity,
    command: &UpdateTask,
    commits: Option<&str>,
    tags: Option<&Tags>,
    record: &TaskRecord,
) -> Result<PreparedTaskUpdate, UpdateTaskError> {
    let new_title = command
        .title
        .as_ref()
        .map_or_else(|| record.title.clone(), ToString::to_string);

    let mut patch = TaskPatch {
        body: compute_body(command, record)?,
        ..TaskPatch::default()
    };
    if command.title.is_some() {
        patch.title.clone_from(&command.title);
    }
    if command.clear_prereq {
        patch.prereq = NullablePatch::Clear;
    } else if let Some(prerequisites) = command.prereq.as_ref() {
        let merged = validate_and_merge(record.prereq.as_deref(), prerequisites, store, projects)
            .map_err(map_prerequisite_error)?;
        patch.prereq = NullablePatch::Set(merged);
    }
    if let Some(commits) = commits {
        patch.commits = NullablePatch::Set(commits.to_string());
    }
    if let Some(effort) = command.effort {
        patch.effort = Some(effort);
    }
    patch.tags = resolve_tags(tags, command.tags_clear, &identity.identifier, record)?;

    let outcome = UpdateTaskOk::OpenTaskEdit {
        id: identity.identifier.clone(),
        project: identity.project.title.to_string(),
        title: new_title,
    };
    Ok(PreparedTaskUpdate {
        identity,
        patch,
        outcome,
    })
}

fn compute_body(
    command: &UpdateTask,
    record: &TaskRecord,
) -> Result<Option<String>, UpdateTaskError> {
    let base = task_body_region(&record.body);
    let mut body = command.prompt.as_deref().map(render);
    if let Some(append) = &command.append {
        let current = body.as_deref().unwrap_or(base);
        body = Some(append_lanes(current, append).ok_or(UpdateTaskError::EmptyAppend)?);
    }
    if let Some(report) = &command.append_report {
        let current = body.as_deref().unwrap_or(base);
        body = Some(append_report_block(current, report).ok_or(UpdateTaskError::EmptyReport)?);
    }
    Ok(body)
}

fn resolve_tags(
    appended: Option<&Tags>,
    clear: bool,
    id: &TaskId,
    record: &TaskRecord,
) -> Result<NullablePatch<Tags>, UpdateTaskError> {
    let Some(appended) = appended else {
        return Ok(if clear {
            NullablePatch::Clear
        } else {
            NullablePatch::Unchanged
        });
    };
    let tags = if clear {
        appended.clone()
    } else if let Some(existing) = record.tags.as_deref() {
        let existing = tags::parse_frontmatter(existing).map_err(|error| {
            UpdateTaskError::InvalidTagsFrontmatter {
                id: id.clone(),
                raw: error.raw().to_string(),
            }
        })?;
        tags::merge(&existing, appended)
    } else {
        appended.clone()
    };
    Ok(NullablePatch::Set(tags))
}

fn amend_closed_task(
    identity: TaskIdentity,
    command: &UpdateTask,
    commits: Option<&str>,
    record: &TaskRecord,
) -> Result<PreparedTaskUpdate, UpdateTaskError> {
    let mut patch = TaskPatch::default();
    let mut changes = Vec::new();
    if let Some(commits) = commits {
        patch.commits = NullablePatch::Set(commits.to_string());
        changes.push(commits_change(commits));
    }
    if let Some(report) = &command.append_report {
        let base = task_body_region(&record.body);
        patch.body = Some(append_report_block(base, report).ok_or(UpdateTaskError::EmptyReport)?);
        changes.push("report appended".to_string());
    }
    let outcome = UpdateTaskOk::Changed {
        id: identity.identifier.clone(),
        changes,
    };
    Ok(PreparedTaskUpdate {
        identity,
        patch,
        outcome,
    })
}

fn commits_change(commits: &str) -> String {
    format!("commits: {commits}")
}

fn map_prerequisite_error(error: PrerequisiteValidationError) -> UpdateTaskError {
    match error {
        PrerequisiteValidationError::UnknownIds { ids } => {
            UpdateTaskError::UnknownPrereqIds { ids }
        }
    }
}

fn parse_tags(values: &[String]) -> Result<Option<Tags>, UpdateTaskError> {
    if values.is_empty() {
        return Ok(None);
    }
    tags::parse_values(values)
        .map(Some)
        .map_err(|error| UpdateTaskError::InvalidTag {
            raw: error.raw().to_string(),
        })
}

#[cfg(test)]
mod tests {
    use pwf_models::task::{TaskStatus, TaskTitle, Timestamp};

    use super::{UpdateTask, UpdateTaskError, UpdateTaskOk};
    use crate::{
        ports::task_record::TaskRecord,
        testing::{InMemoryStore, insert_project, task_record},
    };

    async fn execute(
        command: UpdateTask,
        store: &InMemoryStore,
        pool: &sqlx::SqlitePool,
    ) -> Result<UpdateTaskOk, UpdateTaskError> {
        super::execute(command, store, pool).await
    }

    fn record(id: &str, status: TaskStatus, body: &str) -> TaskRecord {
        TaskRecord {
            status,
            completed: (status != TaskStatus::Active).then(|| Timestamp::new("2026-06-20")),
            body: body.to_string(),
            source: format!("---\nstatus: {status}\n---\n{body}"),
            ..task_record(id)
        }
    }

    fn staged(status: TaskStatus, body: &str) -> InMemoryStore {
        InMemoryStore::default()
            .with_project_id("foo-bar", "FOO")
            .with_project("foo-bar", vec![record("FOO-0001", status, body)])
    }

    fn task_title(raw: &str) -> TaskTitle {
        TaskTitle::try_new(raw).unwrap()
    }

    fn empty(id: &str) -> UpdateTask {
        UpdateTask {
            id: id.parse().unwrap(),
            prompt: None,
            title: None,
            append: None,
            prereq: None,
            clear_prereq: false,
            commits: Vec::new(),
            append_report: None,
            effort: None,
            tags: Vec::new(),
            tags_clear: false,
        }
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn update_rejects_empty_patch_with_nothing_to_update(pool: sqlx::SqlitePool) {
        insert_project(
            &pool,
            "FOO",
            "foo-bar",
            "/projects/foo",
            "/tasks/foo",
            false,
        )
        .await;
        let store = staged(TaskStatus::Active, "## Goals\n- x\n");

        let error = execute(empty("FOO-0001"), &store, &pool).await.unwrap_err();

        assert!(matches!(error, UpdateTaskError::NothingToUpdate));
        assert_eq!(
            error.to_string(),
            "nothing to update (pass --prompt, --title, --prereq, --clear-prereq, --tag, --tags-clear, --commits, --append-report, --append, and/or --effort)."
        );
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn update_allows_commits_amend_on_closed_task(pool: sqlx::SqlitePool) {
        insert_project(
            &pool,
            "FOO",
            "foo-bar",
            "/projects/foo",
            "/tasks/foo",
            false,
        )
        .await;
        let store = staged(TaskStatus::Done, "## Goals\n- x\n");
        let cmd = UpdateTask {
            commits: vec![" abc..def, ghi..jkl ".to_string(), "abc..def".to_string()],
            ..empty("FOO-0001")
        };

        let updated = execute(cmd, &store, &pool).await.unwrap();

        assert_eq!(
            updated,
            UpdateTaskOk::Changed {
                id: "FOO-0001".parse().unwrap(),
                changes: vec!["commits: abc..def, ghi..jkl".to_string()],
            }
        );
        assert_eq!(
            store.tasks("foo-bar")[0].commits.as_deref(),
            Some("abc..def, ghi..jkl")
        );
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn update_rejects_body_edit_on_closed_task(pool: sqlx::SqlitePool) {
        insert_project(
            &pool,
            "FOO",
            "foo-bar",
            "/projects/foo",
            "/tasks/foo",
            false,
        )
        .await;
        let store = staged(TaskStatus::Done, "body\n");
        let cmd = UpdateTask {
            title: Some(task_title("new title")),
            ..empty("FOO-0001")
        };

        let error = execute(cmd, &store, &pool).await.unwrap_err();

        assert!(matches!(
            error,
            UpdateTaskError::ClosedTaskAmendOnly { ref id } if id.as_ref() == "FOO-0001"
        ));
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn update_open_item_edits_title_and_body_and_reports_open_edit(pool: sqlx::SqlitePool) {
        insert_project(
            &pool,
            "FOO",
            "foo-bar",
            "/projects/foo",
            "/tasks/foo",
            false,
        )
        .await;
        let store = staged(TaskStatus::Active, "\nold body\n");
        let cmd = UpdateTask {
            title: Some(task_title("New Title")),
            prompt: Some("fresh prompt".to_string()),
            ..empty("FOO-0001")
        };

        let updated = execute(cmd, &store, &pool).await.unwrap();

        assert_eq!(
            updated,
            UpdateTaskOk::OpenTaskEdit {
                id: "FOO-0001".parse().unwrap(),
                project: "foo-bar".to_string(),
                title: "new title".to_string(),
            }
        );
        let task = &store.tasks("foo-bar")[0];
        assert_eq!(task.title, "new title");
        assert_eq!(task.body, "## Goals\n");
    }

    fn staged_with_tags(raw: &str) -> InMemoryStore {
        let record = TaskRecord {
            tags: Some(raw.to_string()),
            ..record("FOO-0001", TaskStatus::Active, "body\n")
        };
        InMemoryStore::default()
            .with_project_id("foo-bar", "FOO")
            .with_project("foo-bar", vec![record])
    }

    fn tags(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn update_merges_tags_with_existing_frontmatter(pool: sqlx::SqlitePool) {
        insert_project(
            &pool,
            "FOO",
            "foo-bar",
            "/projects/foo",
            "/tasks/foo",
            false,
        )
        .await;
        let store = staged_with_tags("[sqlite, godot]");
        let cmd = UpdateTask {
            tags: tags(&["godot", "csharp-export"]),
            ..empty("FOO-0001")
        };

        execute(cmd, &store, &pool).await.unwrap();

        assert_eq!(
            store.tasks("foo-bar")[0].tags.as_deref(),
            Some("[sqlite, godot, csharp_export]")
        );
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn update_tags_clear_plus_tags_replaces_without_parsing_existing(pool: sqlx::SqlitePool) {
        insert_project(
            &pool,
            "FOO",
            "foo-bar",
            "/projects/foo",
            "/tasks/foo",
            false,
        )
        .await;
        let store = staged_with_tags("still-corrupt");
        let cmd = UpdateTask {
            tags: tags(&["sqlite"]),
            tags_clear: true,
            ..empty("FOO-0001")
        };

        execute(cmd, &store, &pool).await.unwrap();

        assert_eq!(store.tasks("foo-bar")[0].tags.as_deref(), Some("[sqlite]"));
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn update_rejects_corrupt_existing_tags_on_the_merge_path(pool: sqlx::SqlitePool) {
        insert_project(
            &pool,
            "FOO",
            "foo-bar",
            "/projects/foo",
            "/tasks/foo",
            false,
        )
        .await;
        let store = staged_with_tags("sqlite, godot");
        let cmd = UpdateTask {
            tags: tags(&["sqlite"]),
            ..empty("FOO-0001")
        };

        let error = execute(cmd, &store, &pool).await.unwrap_err();

        assert!(matches!(
            error,
            UpdateTaskError::InvalidTagsFrontmatter { ref id, ref raw }
                if id.as_ref() == "FOO-0001" && raw == "sqlite, godot"
        ));
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn update_rejects_invalid_raw_tag_with_cli_display(pool: sqlx::SqlitePool) {
        insert_project(
            &pool,
            "FOO",
            "foo-bar",
            "/projects/foo",
            "/tasks/foo",
            false,
        )
        .await;
        let store = staged(TaskStatus::Active, "body\n");
        let cmd = UpdateTask {
            tags: vec!["sqlite__export".to_string()],
            ..empty("FOO-0001")
        };

        let error = execute(cmd, &store, &pool).await.unwrap_err();

        assert!(matches!(
            error,
            UpdateTaskError::InvalidTag { ref raw } if raw == "sqlite__export"
        ));
        assert_eq!(
            error.to_string(),
            "Invalid --tag value \"sqlite__export\"; use lowercase/uppercase ASCII letters, digits, '_' or '-', without leading, trailing, or repeated separators."
        );
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn update_validates_and_merges_prerequisites_in_the_application(pool: sqlx::SqlitePool) {
        insert_project(
            &pool,
            "FOO",
            "foo-bar",
            "/projects/foo",
            "/tasks/foo",
            false,
        )
        .await;
        let store = InMemoryStore::default()
            .with_project_id("foo-bar", "FOO")
            .with_project(
                "foo-bar",
                vec![
                    TaskRecord {
                        prereq: Some("[[FOO-0001]], [[FOO-0001]]".to_string()),
                        ..record("FOO-0002", TaskStatus::Active, "body\n")
                    },
                    record("FOO-0001", TaskStatus::Done, "body\n"),
                ],
            );
        let cmd = UpdateTask {
            prereq: Some(
                pwf_models::task::Prerequisites::from_inputs(&["foo1, FOO-0001".parse().unwrap()])
                    .unwrap(),
            ),
            ..empty("FOO-0002")
        };

        execute(cmd, &store, &pool).await.unwrap();

        let task = store
            .tasks("foo-bar")
            .into_iter()
            .find(|task| task.id.as_ref() == "FOO-0002")
            .unwrap();
        assert_eq!(task.prereq.as_deref(), Some("[[FOO-0001]]"));
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn update_missing_item_preserves_requested_id(pool: sqlx::SqlitePool) {
        insert_project(
            &pool,
            "FOO",
            "foo-bar",
            "/projects/foo",
            "/tasks/foo",
            false,
        )
        .await;
        let store = staged(TaskStatus::Active, "body\n");
        let cmd = UpdateTask {
            prompt: Some("x".to_string()),
            ..empty("foo-9999")
        };

        let error = execute(cmd, &store, &pool).await.unwrap_err();

        assert!(matches!(
            error,
            UpdateTaskError::TaskNotFound { ref id } if id.as_ref() == "FOO-9999"
        ));
    }
}
