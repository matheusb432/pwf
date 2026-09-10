use pwf_models::project::{Project, ProjectId, ProjectName, ProjectSource, ProjectTasks};

/// Requests one managed project by project ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GetProject {
    /// Project ID.
    pub id: ProjectId,
    /// Project statuses eligible for the lookup.
    pub status: ProjectStatusFilter,
}

impl GetProject {
    #[must_use]
    pub fn new(id: impl Into<ProjectId>, status: ProjectStatusFilter) -> Self {
        Self {
            id: id.into(),
            status,
        }
    }
}

/// Resolves a case-insensitive title before a project ID among eligible projects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveProject {
    pub selector: pwf_models::project::ProjectSelector,
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

/// Requests changes to a managed project's source or Obsidian vault.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateProject {
    /// Project ID.
    pub id: ProjectId,
    /// Replacement source location.
    pub source: crate::patch_field::PatchField<ProjectSource>,
    pub obsidian_vault: crate::patch_field::PatchField<pwf_models::project::ObsidianVault>,
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
    pub source: Option<ProjectSource>,
    pub tasks: ProjectTasks,
    pub obsidian_vault: Option<pwf_models::project::ObsidianVault>,
}

#[derive(Debug, Clone)]
pub struct AddVaultProject {
    pub vault_path: pwf_models::project::ObsidianVault,
    pub id: ProjectId,
    pub tasks_path: pwf_models::project::ProjectTasksRelativePath,
    pub title: Option<ProjectName>,
    pub source_path: Option<pwf_models::project::ProjectSourceValue>,
}
