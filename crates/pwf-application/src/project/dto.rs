use pwf_models::project::{Project, ProjectId, ProjectName, ProjectSource, ProjectTasks};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectStatusFilter(bool);

impl ProjectStatusFilter {
    pub const ACTIVE: Self = Self(false);
    pub const ALL: Self = Self(true);

    pub(crate) const fn includes_paused(self) -> bool {
        self.0
    }
}

/// Describes the current project and whether a requested state transition changed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectStateChange {
    /// Current persisted project.
    pub project: Project,
    /// Reports whether the requested transition changed persisted state.
    pub changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectFields {
    pub id: ProjectId,
    pub title: ProjectName,
    pub source: ProjectSource,
    pub tasks: ProjectTasks,
}

#[derive(Debug)]
pub(super) struct ProjectRow {
    pub(super) id: String,
    pub(super) title: String,
    pub(super) source_kind: String,
    pub(super) source_value: String,
    pub(super) tasks_kind: String,
    pub(super) tasks_path: String,
    pub(super) created_at: String,
    pub(super) is_paused: bool,
}

#[derive(Debug, thiserror::Error)]
#[error("persisted project {field} value {value:?} is invalid: {reason}")]
pub(super) struct ProjectRowError {
    pub(super) field: &'static str,
    pub(super) value: String,
    pub(super) reason: String,
}
