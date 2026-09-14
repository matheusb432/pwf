use std::{collections::BTreeSet, future::Future};

use pwf_models::{
    project::{HomeDirectory, Project, ProjectId},
    task::TaskId,
};
use pwf_wire::project::{
    GetProject, ProjectFields, ProjectStateChange, ProjectStatusFilter, RenameProject,
    UpdateProject,
};

use crate::project::{
    add_project::AddProjectError, get_project::GetProjectError, list_projects::ListProjectsError,
    pause_project::PauseProjectError, rename_project::RenameProjectError,
    resume_project::ResumeProjectError, update_project::UpdateProjectError,
};

#[derive(Debug, thiserror::Error)]
pub enum TaskSequenceError {
    #[error("project {id} does not exist")]
    ProjectNotFound { id: ProjectId },
    #[error("task ID sequence for {id} is exhausted")]
    Exhausted { id: ProjectId },
    #[error("cannot access the task ID sequence: {0}")]
    Unexpected(#[source] anyhow::Error),
}

/// Owns project registry reads and atomic registry mutations.
/// Completed mutations are visible to subsequent reads through the same store.
pub trait ProjectStore: Send + Sync + 'static {
    /// Durably reserves an ID, or returns `None` when existing tasks must seed the sequence.
    /// Reservations are never released, including after a failed file write.
    fn reserve_task_id(
        &self,
        project: &ProjectId,
    ) -> impl Future<Output = Result<Option<TaskId>, TaskSequenceError>> + Send;

    /// Initializes or advances the sequence from authoritative task IDs without lowering it.
    fn advance_task_sequence(
        &self,
        project: &ProjectId,
        highest: Option<&TaskId>,
    ) -> impl Future<Output = Result<(), TaskSequenceError>> + Send;

    fn get_project(
        &self,
        query: GetProject,
    ) -> impl Future<Output = Result<Project, GetProjectError>> + Send;
    fn get_projects(
        &self,
        ids: &BTreeSet<ProjectId>,
    ) -> impl Future<Output = Result<Vec<Project>, GetProjectError>> + Send;
    fn list_projects(
        &self,
        status: ProjectStatusFilter,
    ) -> impl Future<Output = Result<Vec<Project>, ListProjectsError>> + Send;
    fn add_project(
        &self,
        fields: ProjectFields,
        home: &HomeDirectory,
    ) -> impl Future<Output = Result<Project, AddProjectError>> + Send;
    fn update_project(
        &self,
        command: UpdateProject,
    ) -> impl Future<Output = Result<(), UpdateProjectError>> + Send;
    fn pause_project(
        &self,
        project_id: ProjectId,
    ) -> impl Future<Output = Result<ProjectStateChange, PauseProjectError>> + Send;
    fn resume_project(
        &self,
        project_id: ProjectId,
        home: &HomeDirectory,
    ) -> impl Future<Output = Result<ProjectStateChange, ResumeProjectError>> + Send;
    fn rename_project(
        &self,
        command: RenameProject,
        expected: &Project,
        home: &HomeDirectory,
    ) -> impl Future<Output = Result<Project, RenameProjectError>> + Send;
}
