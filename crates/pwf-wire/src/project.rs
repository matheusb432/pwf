use pwf_models::project::{
    Project, ProjectId, ProjectName, ProjectSelector, ProjectSource, ProjectTasks,
};

/// Requests one managed project by project ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GetProject {
    /// Project ID.
    pub id: ProjectId,
    /// Project statuses eligible for the lookup.
    pub status: ProjectStatusFilter,
}

/// Requests replacement of one managed project's identity and locations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameProject {
    /// Existing project ID.
    pub current_id: ProjectId,
    /// Replacement project fields.
    pub fields: ProjectFields,
}

/// Requests one managed-project source update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateProject {
    /// Project ID.
    pub id: ProjectId,
    /// Replacement source location.
    pub source: ProjectSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveProject {
    pub selector: ProjectSelector,
    pub status: ProjectStatusFilter,
}

/// Selects whether paused projects are eligible for a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectStatusFilter {
    /// Includes only active projects.
    ActiveOnly,
    /// Includes active and paused projects.
    IncludingPaused,
}

impl ProjectStatusFilter {
    #[must_use]
    pub const fn includes_paused(self) -> bool {
        matches!(self, Self::IncludingPaused)
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
