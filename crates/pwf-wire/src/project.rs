use pwf_models::project::{Project, ProjectId, ProjectName, ProjectSource, ProjectTasks};

/// Selects whether paused projects are eligible for a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectStatusFilter(bool);

impl ProjectStatusFilter {
    pub const ACTIVE: Self = Self(false);
    pub const ALL: Self = Self(true);

    #[must_use]
    pub const fn includes_paused(self) -> bool {
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

/// Carries the values required to create or replace a managed project's public fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectFields {
    pub id: ProjectId,
    pub title: ProjectName,
    pub source: ProjectSource,
    pub tasks: ProjectTasks,
}
