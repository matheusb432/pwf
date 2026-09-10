use pwf_models::{
    project::Project,
    task::{BlockedBy, TaskId, TaskStatus, TaskTags, TaskTitle, TaskTitleError},
};
use pwf_wire::{
    collection_edit::CollectionEdit,
    patch_field::PatchField,
    set_field::SetField,
    task::{
        EditTask, EditTaskContent, EditTaskContentKind, RawTaskTags, StoredBlockedBy,
        TaskMutationResult, TaskMutationSummary, TaskRecord,
    },
};

use super::{
    TaskPromptTitleError,
    blocked_by::{self, BlockedByValidationError},
    commit_task_writes, ensure_task_revision, expected_task_revision, infer_task_title,
    lane_configuration::{TaskPromptLanes, TaskPromptLanesError},
    note_body::{EditLanesError, append_lanes, edit_lanes, render},
    read_task_dependencies::{self, ReadTaskDependencies, ReadTaskDependenciesError},
    resolve_task_project::{self, ResolveTaskProjectError},
    task_body_region,
};
use crate::ports::task_vault::{
    ExpectedTaskRevision, NullablePatch, TaskMutationError, TaskPatch, TaskVault, TaskWrite,
};

struct PreparedTaskEdit {
    project: Project,
    id: TaskId,
    patch: TaskPatch,
    expected: ExpectedTaskRevision,
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
    #[error(transparent)]
    Revision(#[from] super::TaskRevisionConflict),
    #[error(transparent)]
    InvalidTitle(#[from] TaskPromptTitleError),
    #[error(transparent)]
    PromptLanes(#[from] TaskPromptLanesError),
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
    AmbiguousLanes { header: String },
    #[error(transparent)]
    WriteStore(anyhow::Error),
    #[error(transparent)]
    Mutation(#[from] TaskMutationError<anyhow::Error>),
    #[error(transparent)]
    QueryProject(anyhow::Error),
}

/// Applies content and metadata edits to one active task.
#[cqrsy::command]
pub async fn execute(
    command: EditTask,
    store: &impl TaskVault,
    pool: &sqlx::SqlitePool,
) -> Result<TaskMutationResult<()>, EditTaskError> {
    let project = resolve_task_project::execute(command.id.clone(), pool)
        .await
        .map_err(|error| map_project_error(error, &command.id))?;
    let record = store
        .get_task_record(&project, &command.id)
        .map_err(|error| EditTaskError::WriteStore(anyhow::Error::new(error)))?
        .ok_or_else(|| EditTaskError::TaskNotFound {
            id: command.id.clone(),
        })?;
    ensure_task_revision(command.expected_revision.as_ref(), &record)?;
    if record.status != TaskStatus::Active {
        return Err(EditTaskError::ClosedTask {
            id: command.id.clone(),
        });
    }
    let blocked_by = resolve_blocked_by(command.edits.blocked_by(), &record)?;
    if let (NullablePatch::Set(blockers), Some(supplied)) =
        (&blocked_by, command.edits.blocked_by().addition())
    {
        let dependencies = read_task_dependencies::execute(
            ReadTaskDependencies {
                target: &command.id,
                blockers,
            },
            store,
            pool,
        )
        .await
        .map_err(|error| match error {
            ReadTaskDependenciesError::ReadStore { id, source } => {
                EditTaskError::ReadBlockedBy { id, source }
            }
            ReadTaskDependenciesError::QueryProject(source) => EditTaskError::QueryProject(source),
        })?;
        blocked_by::validate(&command.id, blockers, supplied, &dependencies)
            .map_err(map_blocked_by_error)?;
    }
    let content_patch = match command.edits.content() {
        SetField::Set(content) => {
            let lane_configuration = TaskPromptLanes::load(pool).await?;
            prepare_content(content, &record, &lane_configuration)?
        }
        SetField::NoAction => (SetField::NoAction, SetField::NoAction),
    };
    let prepared = prepare(command, project, &record, blocked_by, content_patch)?;
    let summary = TaskMutationSummary {
        id: prepared.id.clone(),
        title: match prepared.patch.title.as_ref() {
            SetField::NoAction => record.title.clone(),
            SetField::Set(title) => title.to_string(),
        },
        status: record.status,
    };
    persist(prepared, store)?;
    Ok(TaskMutationResult {
        outcome: (),
        task: Some(summary),
    })
}

fn map_project_error(error: ResolveTaskProjectError, id: &TaskId) -> EditTaskError {
    match error {
        ResolveTaskProjectError::UnknownProjectId { .. } => {
            EditTaskError::TaskNotFound { id: id.clone() }
        }
        ResolveTaskProjectError::QueryProject(source) => EditTaskError::QueryProject(source),
    }
}

fn prepare(
    command: EditTask,
    project: Project,
    record: &TaskRecord,
    blocked_by: NullablePatch<BlockedBy>,
    content_patch: (SetField<String>, SetField<TaskTitle>),
) -> Result<PreparedTaskEdit, EditTaskError> {
    let (body, title) = content_patch;
    let patch = TaskPatch {
        body,
        title,
        blocked_by,
        effort: resolve_value(command.edits.effort()),
        priority: resolve_value(command.edits.priority()),
        tags: resolve_tags(command.edits.tags(), &command.id, record)?,
        ..TaskPatch::default()
    };
    Ok(PreparedTaskEdit {
        project,
        id: command.id,
        patch,
        expected: expected_task_revision(record),
    })
}

fn prepare_content(
    content: &EditTaskContent,
    record: &TaskRecord,
    lane_configuration: &TaskPromptLanes,
) -> Result<(SetField<String>, SetField<TaskTitle>), EditTaskError> {
    let current_body = task_body_region(&record.body);
    match content.kind() {
        EditTaskContentKind::Structured { title, lanes } => Ok((
            edit_lanes(current_body, lanes, lane_configuration)
                .map_err(map_lane_error)?
                .into(),
            title.clone(),
        )),
        EditTaskContentKind::AppendShorthand { title, prompt } => Ok((
            SetField::Set(append_lanes(current_body, prompt, lane_configuration)),
            title.clone(),
        )),
        EditTaskContentKind::ReplaceShorthand { prompt } => {
            let title = infer_task_title(prompt, lane_configuration)?;
            Ok((
                SetField::Set(render(prompt, lane_configuration)),
                SetField::Set(title),
            ))
        }
    }
}

fn resolve_blocked_by(
    edit: &CollectionEdit<BlockedBy>,
    record: &TaskRecord,
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
        CollectionEdit::Append(added) => Ok(NullablePatch::Set(
            existing.map_or_else(|| added.clone(), |existing| existing.merge(added)),
        )),
        CollectionEdit::Replace(added) => Ok(NullablePatch::Set(added.clone())),
    }
}

fn map_blocked_by_error(error: BlockedByValidationError) -> EditTaskError {
    match error {
        BlockedByValidationError::UnknownIds { ids } => EditTaskError::UnknownBlockedByIds { ids },
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

fn resolve_value<T: Copy>(edit: &PatchField<T>) -> NullablePatch<T> {
    match edit {
        PatchField::NoAction => NullablePatch::Unchanged,
        PatchField::Set(value) => NullablePatch::Set(*value),
        PatchField::Clear => NullablePatch::Clear,
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
                Some(existing) => merge_appended_tags(id, existing, appended)?,
                None => appended.clone(),
            };
            Ok(NullablePatch::Set(resolved))
        }
    }
}

fn merge_appended_tags(
    id: &TaskId,
    existing: &RawTaskTags,
    appended: &TaskTags,
) -> Result<TaskTags, EditTaskError> {
    let existing =
        pwf_models::task::TaskTags::parse_frontmatter(existing.as_ref()).map_err(|error| {
            EditTaskError::InvalidTagsFrontmatter {
                id: id.clone(),
                raw: error.raw().to_string(),
            }
        })?;
    Ok(existing.merge(appended))
}

fn persist(prepared: PreparedTaskEdit, store: &impl TaskVault) -> Result<(), EditTaskError> {
    commit_task_writes(
        store,
        &prepared.project,
        vec![prepared.expected],
        vec![TaskWrite::Patch {
            id: prepared.id,
            patch: prepared.patch,
        }],
    )
    .map_err(Into::into)
}

fn map_lane_error(error: EditLanesError) -> EditTaskError {
    match error {
        EditLanesError::DuplicateSection { header } => EditTaskError::AmbiguousLanes { header },
    }
}
