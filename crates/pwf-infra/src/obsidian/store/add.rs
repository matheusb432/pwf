use pwf_application::ports::task_vault::TaskInsertion;
use pwf_models::task::TaskId;

use super::{ObsidianStore, ObsidianStoreError};
use crate::obsidian::{
    MarkdownFile,
    note_frontmatter::{NewTaskFields, new_task_content},
};

impl ObsidianStore {
    pub(super) fn write_new_note(
        &self,
        insertion: TaskInsertion<'_>,
    ) -> Result<(), ObsidianStoreError> {
        let (project, id, task) = insertion.into_parts();
        let dir = self.tasks_path(project)?;
        let path = dir.join(format!("{id}.md"));
        if self
            .project_snapshot_path(project)?
            .is_some_and(|snapshot_path| path == snapshot_path)
        {
            return Err(ObsidianStoreError::ProjectSnapshotPathReserved { path });
        }
        let content = new_task_content(NewTaskFields {
            id,
            title: &task.title,
            project: project.title.as_ref(),
            body: &task.body,
            created_at: &task.created_at,
            blocked_by: task.blocked_by.as_ref(),
            effort: task.effort,
            priority: task.priority,
            tags: task.tags.as_ref(),
        });
        let result = MarkdownFile::create_rendered_new(&path, content);
        self.invalidate_task_index(&dir);
        result.map(|_| ()).map_err(|source| {
            map_add_task_error(
                ObsidianStoreError::AddWriteTaskFile {
                    source: source.into_io_error(),
                },
                id,
                &path,
            )
        })
    }
}

fn map_add_task_error(
    error: ObsidianStoreError,
    id: &TaskId,
    path: &std::path::Path,
) -> ObsidianStoreError {
    match error {
        ObsidianStoreError::AddWriteTaskFile { ref source }
            if source.kind() == std::io::ErrorKind::AlreadyExists =>
        {
            ObsidianStoreError::TaskIdOccupied {
                id: id.clone(),
                path: path.to_path_buf(),
            }
        }
        error => error,
    }
}
