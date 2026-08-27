use pwf_models::{
    project::Project,
    task::{BlockedBy, EffortTier, TaskId, TaskStatus, TaskTags, TaskTitle, TaskTitleError},
};
use pwf_wire::{
    project::ProjectStatusFilter,
    task::{CollectionEdit, EditTask, EditTaskContent, EditTaskContentKind, EditedTask, ValueEdit},
};

use super::{
    blocked_by::{self, BlockedByValidationError, validate_and_merge},
    note_body::{EditLanesError, append_lanes, edit_lanes, render},
    resolve_task_project::{self, ResolveTaskProjectError},
    tags, task_body_region,
};
use crate::{
    ports::task_record::{NullablePatch, StoredBlockedBy, TaskPatch, TaskRecord, TaskStore},
    project::list_projects,
};

struct TaskIdentity {
    project: Project,
    id: TaskId,
}

struct PreparedTaskEdit {
    identity: TaskIdentity,
    patch: TaskPatch,
    outcome: EditedTask,
}

#[derive(Debug, thiserror::Error)]
pub enum EditTaskError {
    #[error("Task not found: {id}")]
    TaskNotFound { id: TaskId },
    #[error("cannot edit closed task {id}; run `pwf task reopen {id}` first.")]
    ClosedTask { id: TaskId },
    #[error("task {id} has an invalid persisted title: {source}")]
    InvalidPersistedTitle {
        id: TaskId,
        #[source]
        source: TaskTitleError,
    },
    #[error("task {id} has invalid tags frontmatter: {raw:?}.")]
    InvalidTagsFrontmatter { id: TaskId, raw: String },
    #[error("task {id} at {path} has malformed blocked_by metadata {raw:?}: {reason}")]
    MalformedBlockedBy {
        id: TaskId,
        path: Box<pwf_wire::task::TaskNotePath>,
        raw: Box<str>,
        reason: Box<str>,
    },
    #[error(
        "Unknown --add-blocked-by id(s): {}.",
        blocked_by::format_task_ids(ids)
    )]
    UnknownBlockedByIds { ids: Vec<TaskId> },
    #[error("cannot validate --add-blocked-by task {id}: {source}")]
    ReadBlockedBy {
        id: TaskId,
        #[source]
        source: anyhow::Error,
    },
    #[error("task {target} cannot be blocked by itself ({blocker})")]
    SelfBlockedBy { target: TaskId, blocker: TaskId },
    #[error("blocked_by cycle: {}", blocked_by::format_task_ids_path(path))]
    BlockedByCycle { path: Vec<TaskId> },
    #[error("cannot edit lanes: task body contains more than one `{header}` section.")]
    AmbiguousLanes { header: &'static str },
    #[error(transparent)]
    WriteStore(anyhow::Error),
    #[error(transparent)]
    QueryProject(anyhow::Error),
}

/// Applies content and metadata edits to one active task.
#[cqrsy::command]
pub async fn execute(
    command: EditTask,
    store: &impl TaskStore,
    pool: &sqlx::SqlitePool,
) -> Result<EditedTask, EditTaskError> {
    let project = resolve_task_project::execute(command.id.clone(), pool)
        .await
        .map_err(|error| map_project_error(error, &command.id))?;
    let record = store
        .get(&project, &command.id)
        .map_err(|error| EditTaskError::WriteStore(anyhow::Error::new(error)))?
        .ok_or_else(|| EditTaskError::TaskNotFound {
            id: command.id.clone(),
        })?;
    if record.status != TaskStatus::Active {
        return Err(EditTaskError::ClosedTask {
            id: command.id.clone(),
        });
    }

    let projects = resolve_blocked_by_projects(&command, pool).await?;
    let prepared = prepare(command, project, &record, &projects, store)?;
    persist(prepared, store)
}

fn map_project_error(error: ResolveTaskProjectError, id: &TaskId) -> EditTaskError {
    match error {
        ResolveTaskProjectError::UnknownProjectId { .. } => {
            EditTaskError::TaskNotFound { id: id.clone() }
        }
        ResolveTaskProjectError::QueryProject(source) => EditTaskError::QueryProject(source),
    }
}

async fn resolve_blocked_by_projects(
    command: &EditTask,
    pool: &sqlx::SqlitePool,
) -> Result<Vec<Project>, EditTaskError> {
    if command.edits.blocked_by().addition().is_none() {
        return Ok(Vec::new());
    }
    list_projects::execute(ProjectStatusFilter::IncludingPaused, pool)
        .await
        .map_err(|error| EditTaskError::QueryProject(anyhow::Error::new(error)))
}

fn prepare(
    command: EditTask,
    project: Project,
    record: &TaskRecord,
    projects: &[Project],
    store: &impl TaskStore,
) -> Result<PreparedTaskEdit, EditTaskError> {
    let blocked_by = resolve_blocked_by(command.edits.blocked_by(), record, store, projects)?;
    let identity = TaskIdentity {
        project,
        id: command.id.clone(),
    };
    let (body, title, outcome_title) =
        prepare_content(command.edits.content(), record, &command.id)?;
    let patch = TaskPatch {
        body,
        title,
        blocked_by,
        effort: resolve_effort(command.edits.effort()),
        tags: resolve_tags(command.edits.tags(), &command.id, record)?,
        ..TaskPatch::default()
    };
    let outcome = EditedTask {
        id: command.id,
        project: identity.project.title.clone(),
        title: outcome_title,
    };
    Ok(PreparedTaskEdit {
        identity,
        patch,
        outcome,
    })
}

fn prepare_content(
    content: Option<&EditTaskContent>,
    record: &TaskRecord,
    id: &TaskId,
) -> Result<(Option<String>, Option<TaskTitle>, TaskTitle), EditTaskError> {
    let current_body = task_body_region(&record.body);
    let current_title = || {
        TaskTitle::try_new(record.title.clone()).map_err(|source| {
            EditTaskError::InvalidPersistedTitle {
                id: id.clone(),
                source,
            }
        })
    };
    match content.map(EditTaskContent::kind) {
        None => Ok((None, None, current_title()?)),
        Some(EditTaskContentKind::Structured { title, lanes }) => Ok((
            edit_lanes(current_body, lanes).map_err(map_lane_error)?,
            title.clone(),
            title.clone().map_or_else(current_title, Ok)?,
        )),
        Some(EditTaskContentKind::AppendShorthand { title, prompt }) => Ok((
            Some(append_lanes(current_body, prompt)),
            title.clone(),
            title.clone().map_or_else(current_title, Ok)?,
        )),
        Some(EditTaskContentKind::ReplaceShorthand { prompt, title }) => {
            Ok((Some(render(prompt)), Some(title.clone()), title.clone()))
        }
    }
}

fn resolve_blocked_by(
    edit: &CollectionEdit<BlockedBy>,
    record: &TaskRecord,
    store: &impl TaskStore,
    projects: &[Project],
) -> Result<NullablePatch<BlockedBy>, EditTaskError> {
    let existing = match &record.blocked_by {
        StoredBlockedBy::Absent => None,
        StoredBlockedBy::Valid(blocked_by) => Some(blocked_by),
        StoredBlockedBy::Malformed { raw, reason } => {
            return Err(EditTaskError::MalformedBlockedBy {
                id: record.id.clone(),
                path: Box::new(record.locator.clone()),
                raw: raw.clone().into_boxed_str(),
                reason: reason.clone().into_boxed_str(),
            });
        }
    };
    match edit {
        CollectionEdit::Unchanged => Ok(NullablePatch::Unchanged),
        CollectionEdit::Clear => Ok(NullablePatch::Clear),
        CollectionEdit::Append(added) => {
            validate_and_merge(&record.id, existing, added, store, projects)
                .map(NullablePatch::Set)
                .map_err(map_blocked_by_error)
        }
        CollectionEdit::Replace(added) => {
            validate_and_merge(&record.id, None, added, store, projects)
                .map(NullablePatch::Set)
                .map_err(map_blocked_by_error)
        }
    }
}

fn map_blocked_by_error(error: BlockedByValidationError) -> EditTaskError {
    match error {
        BlockedByValidationError::UnknownIds { ids } => EditTaskError::UnknownBlockedByIds { ids },
        BlockedByValidationError::ReadStore { id, source } => {
            EditTaskError::ReadBlockedBy { id, source }
        }
        BlockedByValidationError::SelfDependency { target, blocker } => {
            EditTaskError::SelfBlockedBy { target, blocker }
        }
        BlockedByValidationError::Cycle { path } => EditTaskError::BlockedByCycle { path },
        BlockedByValidationError::MalformedMetadata {
            task,
            path,
            raw,
            reason,
        } => EditTaskError::MalformedBlockedBy {
            id: task,
            path,
            raw,
            reason,
        },
    }
}

fn resolve_effort(edit: &ValueEdit<EffortTier>) -> NullablePatch<EffortTier> {
    match edit {
        ValueEdit::Unchanged => NullablePatch::Unchanged,
        ValueEdit::Set(effort) => NullablePatch::Set(*effort),
        ValueEdit::Clear => NullablePatch::Clear,
    }
}

fn resolve_tags(
    edit: &CollectionEdit<TaskTags>,
    id: &TaskId,
    record: &TaskRecord,
) -> Result<NullablePatch<TaskTags>, EditTaskError> {
    match edit {
        CollectionEdit::Unchanged => Ok(NullablePatch::Unchanged),
        CollectionEdit::Clear => Ok(NullablePatch::Clear),
        CollectionEdit::Replace(tags) => Ok(NullablePatch::Set(tags.clone())),
        CollectionEdit::Append(appended) => {
            let resolved = match record.tags.as_ref() {
                Some(existing) => {
                    let existing = tags::parse_frontmatter(existing).map_err(|error| {
                        EditTaskError::InvalidTagsFrontmatter {
                            id: id.clone(),
                            raw: error.raw().to_string(),
                        }
                    })?;
                    existing.merge(appended)
                }
                None => appended.clone(),
            };
            Ok(NullablePatch::Set(resolved))
        }
    }
}

fn persist(
    prepared: PreparedTaskEdit,
    store: &impl TaskStore,
) -> Result<EditedTask, EditTaskError> {
    store
        .update(
            &prepared.identity.project,
            &prepared.identity.id,
            prepared.patch,
        )
        .map_err(|error| EditTaskError::WriteStore(anyhow::Error::new(error)))?;
    Ok(prepared.outcome)
}

fn map_lane_error(error: EditLanesError) -> EditTaskError {
    match error {
        EditLanesError::DuplicateSection { header } => EditTaskError::AmbiguousLanes { header },
    }
}

#[cfg(test)]
mod tests {
    use pwf_models::task::{BlockedBy, EffortTier, TaskPrompt, TaskStatus, TaskTags, TaskTitle};
    use pwf_wire::task::{
        CollectionEdit, EditTask, EditTaskContent, EditedTask, RawTaskTags, TaskEdits, TaskLane,
        TaskLaneEdits, TaskLanes, ValueEdit,
    };

    use super::EditTaskError;
    use crate::{
        ports::task_record::TaskRecord,
        task::edit_task,
        testing::{InMemoryStore, app_date, insert_project, stored_blocked_by, task_record},
    };

    fn record(id: &str, status: TaskStatus, body: &str) -> TaskRecord {
        TaskRecord {
            status,
            completed: (status != TaskStatus::Active).then(|| app_date("2026-06-20")),
            body: body.to_string(),
            source: format!("---\nstatus: {status}\n---\n{body}"),
            ..task_record(id)
        }
    }

    fn staged(records: Vec<TaskRecord>) -> InMemoryStore {
        InMemoryStore::default()
            .with_project_id("foo-bar", "FOO")
            .with_project("foo-bar", records)
    }

    fn edit(
        id: &str,
        content: Option<EditTaskContent>,
        blocked_by: CollectionEdit<BlockedBy>,
        effort: ValueEdit<EffortTier>,
        tags: CollectionEdit<TaskTags>,
    ) -> EditTask {
        EditTask {
            id: id.parse().unwrap(),
            edits: TaskEdits::try_new(content, blocked_by, effort, tags).unwrap(),
        }
    }

    fn content_edit(id: &str, content: EditTaskContent) -> EditTask {
        edit(
            id,
            Some(content),
            CollectionEdit::Unchanged,
            ValueEdit::Unchanged,
            CollectionEdit::Unchanged,
        )
    }

    fn title(raw: &str) -> TaskTitle {
        TaskTitle::try_new(raw).unwrap()
    }

    fn tags(raw: &str) -> TaskTags {
        TaskTags::from_inputs(&[raw.parse().unwrap()]).unwrap()
    }

    async fn run(
        command: EditTask,
        store: &InMemoryStore,
        pool: &sqlx::SqlitePool,
    ) -> Result<EditedTask, EditTaskError> {
        edit_task::execute(command, store, pool).await
    }

    async fn register_project(pool: &sqlx::SqlitePool) {
        insert_project(pool, "FOO", "foo-bar", "/projects/foo", "/tasks/foo", false).await;
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn edit_rejects_closed_tasks_before_content_changes(pool: sqlx::SqlitePool) {
        register_project(&pool).await;
        let store = staged(vec![record(
            "FOO-0001",
            TaskStatus::Done,
            "## Goals\n\n- keep this",
        )]);
        let command = content_edit(
            "FOO-0001",
            EditTaskContent::structured(Some(title("new title")), TaskLaneEdits::default())
                .unwrap(),
        );

        let error = run(command, &store, &pool).await.unwrap_err();

        assert!(matches!(
            error,
            EditTaskError::ClosedTask { ref id } if id.as_ref() == "FOO-0001"
        ));
        assert_eq!(store.tasks("foo-bar")[0].title, "tray gui");
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn prompt_replaces_title_and_every_lane(pool: sqlx::SqlitePool) {
        register_project(&pool).await;
        let store = staged(vec![record(
            "FOO-0001",
            TaskStatus::Active,
            "## Goals\n\n- old\n\n## Context\n\n- old context",
        )]);
        let command = content_edit(
            "FOO-0001",
            EditTaskContent::replace_shorthand(TaskPrompt::new("New Title / new goal /d complete"))
                .unwrap(),
        );

        let outcome = run(command, &store, &pool).await.unwrap();

        assert_eq!(outcome.title.as_ref(), "new title");
        let edited = &store.tasks("foo-bar")[0];
        assert_eq!(edited.title, "new title");
        assert_eq!(
            edited.body,
            "## Goals\n\n- new goal\n\n## Done When\n\n- complete"
        );
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn structured_content_replaces_lanes_and_preserves_unrelated_markdown(
        pool: sqlx::SqlitePool,
    ) {
        register_project(&pool).await;
        let store = staged(vec![record(
            "FOO-0001",
            TaskStatus::Active,
            "## Goals\n\n- old\n\n## Notes\n\nkeep me\n\n## Done When\n\n- old outcome",
        )]);
        let additions = TaskLanes::try_new(
            vec!["new /c literal".to_string()],
            vec!["new context".to_string()],
            Vec::new(),
            vec!["new outcome".to_string()],
        )
        .unwrap();
        let command = content_edit(
            "FOO-0001",
            EditTaskContent::structured(
                None,
                TaskLaneEdits::new(
                    additions,
                    [TaskLane::Goal, TaskLane::Context, TaskLane::DoneWhen],
                ),
            )
            .unwrap(),
        );

        run(command, &store, &pool).await.unwrap();

        assert_eq!(
            store.tasks("foo-bar")[0].body,
            "## Goals\n\n- new /c literal\n\n## Notes\n\nkeep me\n\n## Context\n\n- new context\n\n## Done When\n\n- new outcome"
        );
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn structured_content_rejects_duplicate_lane_headings(pool: sqlx::SqlitePool) {
        register_project(&pool).await;
        let body = "## Goals\n\n- one\n\n## Goals\n\n- two";
        let store = staged(vec![record("FOO-0001", TaskStatus::Active, body)]);
        let command = content_edit(
            "FOO-0001",
            EditTaskContent::structured(
                None,
                TaskLaneEdits::new(TaskLanes::default(), [TaskLane::Goal]),
            )
            .unwrap(),
        );

        let error = run(command, &store, &pool).await.unwrap_err();

        assert!(matches!(
            error,
            EditTaskError::AmbiguousLanes { header: "## Goals" }
        ));
        assert_eq!(store.tasks("foo-bar")[0].body, body);
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn append_can_replace_the_title_without_reparsing_it(pool: sqlx::SqlitePool) {
        register_project(&pool).await;
        let store = staged(vec![record(
            "FOO-0001",
            TaskStatus::Active,
            "## Goals\n\n- old",
        )]);
        let command = content_edit(
            "FOO-0001",
            EditTaskContent::append_shorthand(
                Some(title("renamed")),
                TaskPrompt::new("additional /c context"),
            )
            .unwrap(),
        );

        run(command, &store, &pool).await.unwrap();

        let edited = &store.tasks("foo-bar")[0];
        assert_eq!(edited.title, "renamed");
        assert_eq!(
            edited.body,
            "## Goals\n\n- old\n- additional\n\n## Context\n\n- context\n"
        );
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn remove_then_add_replaces_tags_blocked_by_and_effort(pool: sqlx::SqlitePool) {
        register_project(&pool).await;
        let target = TaskRecord {
            tags: Some(RawTaskTags::new("[old]")),
            blocked_by: stored_blocked_by(&["FOO-0001"]),
            effort: Some("medium".to_string()),
            ..record("FOO-0002", TaskStatus::Active, "## Goals\n")
        };
        let store = staged(vec![record("FOO-0001", TaskStatus::Done, "body"), target]);
        let command = edit(
            "FOO-0002",
            None,
            CollectionEdit::Replace(crate::testing::blocked_by(&["FOO-0001"])),
            ValueEdit::Set(EffortTier::High),
            CollectionEdit::Replace(tags("new-tag")),
        );

        run(command, &store, &pool).await.unwrap();

        let edited = store
            .tasks("foo-bar")
            .into_iter()
            .find(|task| task.id.as_ref() == "FOO-0002")
            .unwrap();
        assert_eq!(edited.tags.as_ref().map(AsRef::as_ref), Some("[new_tag]"));
        assert_eq!(
            edited
                .blocked_by
                .valid()
                .map(|value| value.iter().map(AsRef::as_ref).collect::<Vec<_>>()),
            Some(vec!["FOO-0001"])
        );
        assert_eq!(edited.effort.as_deref(), Some("high"));
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn blocked_by_edit_rejects_a_multihop_cycle_without_mutating_the_task(
        pool: sqlx::SqlitePool,
    ) {
        register_project(&pool).await;
        let first = TaskRecord {
            blocked_by: stored_blocked_by(&["FOO-0002"]),
            ..record("FOO-0001", TaskStatus::Done, "first")
        };
        let second = TaskRecord {
            blocked_by: stored_blocked_by(&["FOO-0003"]),
            ..record("FOO-0002", TaskStatus::Cancelled, "second")
        };
        let target = record("FOO-0003", TaskStatus::Active, "target");
        let store = staged(vec![first, second, target]);
        let before = store.tasks("foo-bar");
        let command = edit(
            "FOO-0003",
            None,
            CollectionEdit::Append(crate::testing::blocked_by(&["FOO-0001"])),
            ValueEdit::Unchanged,
            CollectionEdit::Unchanged,
        );

        let error = run(command, &store, &pool).await.unwrap_err();

        assert_eq!(
            error.to_string(),
            "blocked_by cycle: FOO-0003 -> FOO-0001 -> FOO-0002 -> FOO-0003"
        );
        assert!(matches!(
            &error,
            EditTaskError::BlockedByCycle { path }
                if path.iter().map(AsRef::as_ref).collect::<Vec<_>>()
                    == ["FOO-0003", "FOO-0001", "FOO-0002", "FOO-0003"]
        ));
        assert_eq!(store.tasks("foo-bar"), before);
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn remove_effort_clears_existing_effort(pool: sqlx::SqlitePool) {
        register_project(&pool).await;
        let store = staged(vec![TaskRecord {
            effort: Some("medium".to_string()),
            ..record("FOO-0001", TaskStatus::Active, "## Goals\n")
        }]);
        let command = edit(
            "FOO-0001",
            None,
            CollectionEdit::Unchanged,
            ValueEdit::Clear,
            CollectionEdit::Unchanged,
        );

        run(command, &store, &pool).await.unwrap();

        assert_eq!(store.tasks("foo-bar")[0].effort, None);
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn metadata_edit_rejects_an_invalid_persisted_title_before_mutation(
        pool: sqlx::SqlitePool,
    ) {
        register_project(&pool).await;
        let store = staged(vec![TaskRecord {
            title: "x".repeat(201),
            ..record("FOO-0001", TaskStatus::Active, "## Goals\n")
        }]);
        let command = edit(
            "FOO-0001",
            None,
            CollectionEdit::Unchanged,
            ValueEdit::Set(EffortTier::High),
            CollectionEdit::Unchanged,
        );

        let error = run(command, &store, &pool).await.unwrap_err();

        assert!(matches!(error, EditTaskError::InvalidPersistedTitle { .. }));
        assert_eq!(store.tasks("foo-bar")[0].effort, None);
    }
}
