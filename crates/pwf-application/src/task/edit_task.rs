use pwf_models::{
    project::ProjectId,
    task::{BlockedBy, TaskId, TaskTags, TaskTitle, TaskTitleError},
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
    TaskBodyTitleError,
    blocked_by::{self, BlockedByValidationError},
    commit_task_writes,
    content::{append_marker_sections, render_for_replacement},
    ensure_task_revision, expected_task_revision, infer_task_title,
    read_task_dependencies::{self, ReadTaskDependencies, ReadTaskDependenciesError},
    resolve_task_project::{self, ResolveTaskProjectError},
    task_body_region,
};
use crate::ports::{
    project_store::ProjectStore,
    task_vault::{NullablePatch, TaskMutationError, TaskPatch, TaskVault, TaskWrite},
    user_settings::{TaskBodyPresetReader, UserSettingsLoadError},
};

#[derive(Debug, thiserror::Error)]
pub enum EditTaskError {
    #[error("Task not found: {id}")]
    TaskNotFound { id: TaskId },
    #[error("cannot edit closed task {id}; run `pwf task activate {id}` first.")]
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
    InvalidTitle(#[from] TaskBodyTitleError),
    #[error(transparent)]
    TaskBodyPresets(#[from] UserSettingsLoadError),
    #[error("task {id} has invalid tags frontmatter: {raw:?}.")]
    InvalidTagsFrontmatter { id: TaskId, raw: String },
    #[error("task {id} at {path} has malformed blocked_by metadata {raw:?}: {reason}")]
    MalformedBlockedBy {
        id: TaskId,
        path: Box<pwf_wire::task::TaskFilePath>,
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
    #[error(transparent)]
    WriteStore(anyhow::Error),
    #[error(transparent)]
    Mutation(#[from] TaskMutationError<anyhow::Error>),
    #[error(transparent)]
    QueryProject(anyhow::Error),
}

/// Applies content and metadata edits to one active or backlogged task.
#[cqrsy::command]
pub async fn execute(
    command: EditTask,
    store: &impl TaskVault,
    project_store: &impl ProjectStore,
    preset_reader: &impl TaskBodyPresetReader,
) -> Result<TaskMutationResult<()>, EditTaskError> {
    let project = resolve_task_project::execute(command.id.clone(), project_store)
        .await
        .map_err(|error| map_project_error(error, &command.id))?;
    let record = store
        .get_task_record(&project, &command.id)
        .map_err(|error| EditTaskError::WriteStore(anyhow::Error::new(error)))?
        .ok_or_else(|| EditTaskError::TaskNotFound {
            id: command.id.clone(),
        })?;
    ensure_task_revision(command.expected_revision.as_ref(), &record)?;
    if record.status.is_closed() {
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
            project_store,
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
    let (body, title) = match command.edits.content() {
        SetField::Set(content) => prepare_content(content, &record, &project.id, preset_reader)?,
        SetField::NoAction => (SetField::NoAction, SetField::NoAction),
    };
    let patch = TaskPatch {
        body,
        title,
        blocked_by,
        effort: resolve_value(command.edits.effort()),
        priority: resolve_value(command.edits.priority()),
        tags: resolve_tags(command.edits.tags(), &command.id, &record)?,
        ..TaskPatch::default()
    };
    let summary = TaskMutationSummary {
        id: command.id.clone(),
        title: match patch.title.as_ref() {
            SetField::NoAction => record.title.clone(),
            SetField::Set(title) => title.to_string(),
        },
        status: record.status,
    };
    commit_task_writes(
        store,
        &project,
        vec![expected_task_revision(&record)],
        vec![TaskWrite::Patch {
            id: command.id,
            patch,
        }],
    )?;
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

/// Loads the project's preset only for edits that parse shorthand.
fn prepare_content(
    content: &EditTaskContent,
    record: &TaskRecord,
    project_id: &ProjectId,
    preset_reader: &impl TaskBodyPresetReader,
) -> Result<(SetField<String>, SetField<TaskTitle>), EditTaskError> {
    let current_body = task_body_region(&record.body);
    match content.kind() {
        EditTaskContentKind::Title(title) => Ok((SetField::NoAction, SetField::Set(title.clone()))),
        EditTaskContentKind::AppendShorthand { title, body } => {
            let presets = preset_reader.load_task_body_presets()?;
            Ok((
                SetField::Set(append_marker_sections(
                    current_body,
                    body,
                    presets.for_project(project_id),
                )),
                title.clone(),
            ))
        }
        EditTaskContentKind::ReplaceShorthand { body } => {
            let presets = preset_reader.load_task_body_presets()?;
            let preset = presets.for_project(project_id);
            let title = infer_task_title(body, preset)?;
            Ok((
                SetField::Set(render_for_replacement(body, preset)),
                SetField::Set(title),
            ))
        }
    }
}

fn resolve_blocked_by(
    edit: &CollectionEdit<BlockedBy>,
    record: &TaskRecord,
) -> Result<NullablePatch<BlockedBy>, EditTaskError> {
    match edit {
        CollectionEdit::Unchanged => Ok(NullablePatch::Unchanged),
        CollectionEdit::Clear => Ok(NullablePatch::Clear),
        CollectionEdit::Replace(added) => Ok(NullablePatch::Set(added.clone())),
        CollectionEdit::Append(added) => match &record.blocked_by {
            StoredBlockedBy::Absent => Ok(NullablePatch::Set(added.clone())),
            StoredBlockedBy::Valid(existing) => Ok(NullablePatch::Set(existing.merge(added))),
            StoredBlockedBy::Malformed { raw, reason } => Err(EditTaskError::MalformedBlockedBy {
                id: record.id.clone(),
                path: Box::new(record.locator.clone()),
                raw: raw.clone().into_boxed_str(),
                reason: reason.clone().into_boxed_str(),
            }),
        },
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
