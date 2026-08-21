use std::path::PathBuf;

use pwf_models::project::{
    Project, ProjectId, ProjectName, ProjectSelector, ProjectSource, ProjectTasks, ProjectTasksPath,
};

/// Requests creation of one managed project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddProject {
    /// Project fields parsed from user input.
    pub fields: ProjectFields,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AddProjectApiError {
    #[error("resolving the home directory for managed projects failed")]
    HomeDirectoryUnavailable,
    #[error("project id already exists: {id}")]
    DuplicateProjectId { id: ProjectId },
    #[error("project title already exists: {title}")]
    DuplicateProjectTitle { title: ProjectName },
    #[error(transparent)]
    TaskLocation(#[from] ProjectTaskLocationApiError),
    #[error("{message}")]
    Unexpected { message: String },
    #[error("rendering project JSON failed: {message}")]
    RenderJson { message: String },
}

/// Requests one active managed project by project ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GetActiveProject {
    pub id: ProjectId,
}

/// Requests one managed project by project ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GetProject {
    /// Project ID.
    pub id: ProjectId,
    /// Project statuses eligible for the lookup.
    pub status: ProjectStatusFilter,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GetProjectApiError {
    #[error("project not found: {id}")]
    ProjectNotFound { id: ProjectId },
    #[error("{message}")]
    Unexpected { message: String },
    #[error("rendering project JSON failed: {message}")]
    RenderJson { message: String },
}

/// Requests managed projects in ascending title order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListProjects {
    /// Project statuses eligible for the list.
    pub status: ProjectStatusFilter,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ListProjectsApiError {
    #[error("{message}")]
    Unexpected { message: String },
    #[error("rendering project JSON failed: {message}")]
    RenderJson { message: String },
}

/// Requests pausing one managed project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PauseProject {
    /// Project ID.
    pub id: ProjectId,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PauseProjectApiError {
    #[error("project not found: {id}")]
    ProjectNotFound { id: ProjectId },
    #[error("{message}")]
    Unexpected { message: String },
    #[error("rendering project JSON failed: {message}")]
    RenderJson { message: String },
}

/// Requests replacement of one managed project's identity and locations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameProject {
    /// Existing project ID.
    pub current_id: ProjectId,
    /// Replacement project fields.
    pub fields: ProjectFields,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RenameProjectApiError {
    #[error("resolving the home directory for managed projects failed")]
    HomeDirectoryUnavailable,
    #[error("project rename failed: project not found: {id}")]
    SourceProjectNotFound { id: ProjectId },
    #[error("project rename failed: project changed while task files were staged: {id}")]
    SourceProjectChanged { id: ProjectId },
    #[error("project rename failed: project id already exists: {id}")]
    DestinationProjectIdExists { id: ProjectId },
    #[error("project rename failed: project title already exists: {title}")]
    DestinationProjectTitleExists { title: ProjectName },
    #[error("project rename failed: {0}")]
    TaskLocation(#[from] ProjectTaskLocationApiError),
    #[error("{message}")]
    Unexpected { message: String },
    #[error("rendering project JSON failed: {message}")]
    RenderJson { message: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveProject {
    pub selector: ProjectSelector,
    pub status: ProjectStatusFilter,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ResolveProjectApiError {
    #[error(
        "Unknown managed project identifier: {selector}\nManaged project identifiers: {}",
        format_project_names(known)
    )]
    Unknown {
        selector: ProjectSelector,
        known: Vec<ProjectName>,
    },
    #[error("{message}")]
    Unexpected { message: String },
}

/// Requests resuming one managed project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumeProject {
    /// Project ID.
    pub id: ProjectId,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ResumeProjectApiError {
    #[error("resolving the home directory for managed projects failed")]
    HomeDirectoryUnavailable,
    #[error("project not found: {id}")]
    ProjectNotFound { id: ProjectId },
    #[error(transparent)]
    TaskLocation(#[from] ProjectTaskLocationApiError),
    #[error("{message}")]
    Unexpected { message: String },
    #[error("rendering project JSON failed: {message}")]
    RenderJson { message: String },
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProjectTaskLocationApiError {
    #[error("managed project {project_id} task path '{path}' is invalid: {reason}")]
    InvalidTaskPath {
        project_id: ProjectId,
        path: ProjectTasksPath,
        reason: String,
    },
    #[error(
        "managed projects {first_id} and {second_id} resolve to the same task location: {}",
        path.display()
    )]
    DuplicateRuntimeTaskLocation {
        first_id: ProjectId,
        second_id: ProjectId,
        path: PathBuf,
    },
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

pub(crate) fn format_project_names(names: &[ProjectName]) -> String {
    names
        .iter()
        .map(AsRef::as_ref)
        .collect::<Vec<_>>()
        .join(", ")
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
