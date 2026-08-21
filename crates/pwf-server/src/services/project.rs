use pwf_application::project::{
    TaskLocationError,
    add_project::{self, AddProjectError},
    get_project::{self, GetProjectError},
    list_projects::{self, ListProjectsError},
    pause_project::{self, PauseProjectError},
    rename_project::{self, RenameProjectError},
    resolve_project::ResolveProjectError,
    resume_project::{self, ResumeProjectError},
};
use pwf_infra::obsidian::ObsidianProjectTaskFilesClient;
use pwf_wire::v1::{self, project_service_server::ProjectService};
use tonic::{Request, Response, Status};

use crate::{
    AppState,
    conversion::{input, output},
};

#[derive(Clone)]
pub(crate) struct ProjectApi {
    state: AppState,
}

impl ProjectApi {
    pub(crate) fn new(state: AppState) -> Self {
        Self { state }
    }
}

#[tonic::async_trait]
impl ProjectService for ProjectApi {
    async fn add_project(
        &self,
        request: Request<v1::AddProjectRequest>,
    ) -> Result<Response<v1::Project>, Status> {
        let command = input::add_project(request.into_inner())?;
        add_project::execute(command, &self.state.pool, &self.state.home)
            .await
            .map(|project| output::project(&project))
            .map(Response::new)
            .map_err(add_project_status)
    }

    async fn get_project(
        &self,
        request: Request<v1::GetProjectRequest>,
    ) -> Result<Response<v1::Project>, Status> {
        let query = input::get_project(request.into_inner())?;
        get_project::execute(query, &self.state.pool)
            .await
            .map(|project| output::project(&project))
            .map(Response::new)
            .map_err(|error| get_project_status(&error))
    }

    async fn list_projects(
        &self,
        request: Request<v1::ListProjectsRequest>,
    ) -> Result<Response<v1::ListProjectsResponse>, Status> {
        let query = input::list_projects(request.into_inner())?;
        list_projects::execute(query, &self.state.pool)
            .await
            .map(|projects| v1::ListProjectsResponse {
                projects: projects.iter().map(output::project).collect(),
            })
            .map(Response::new)
            .map_err(|error| list_projects_status(&error))
    }

    async fn pause_project(
        &self,
        request: Request<v1::PauseProjectRequest>,
    ) -> Result<Response<v1::ProjectStateChange>, Status> {
        let command = input::pause_project(request.into_inner())?;
        pause_project::execute(command, &self.state.pool)
            .await
            .map(|change| output::project_state_change(&change))
            .map(Response::new)
            .map_err(|error| pause_project_status(&error))
    }

    async fn rename_project(
        &self,
        request: Request<v1::RenameProjectRequest>,
    ) -> Result<Response<v1::Project>, Status> {
        let command = input::rename_project(request.into_inner())?;
        rename_project::execute(
            command,
            &self.state.pool,
            &ObsidianProjectTaskFilesClient,
            &self.state.home,
        )
        .await
        .map(|project| output::project(&project))
        .map(Response::new)
        .map_err(rename_project_status)
    }

    async fn resume_project(
        &self,
        request: Request<v1::ResumeProjectRequest>,
    ) -> Result<Response<v1::ProjectStateChange>, Status> {
        let command = input::resume_project(request.into_inner())?;
        resume_project::execute(command, &self.state.pool, &self.state.home)
            .await
            .map(|change| output::project_state_change(&change))
            .map(Response::new)
            .map_err(resume_project_status)
    }
}

pub(super) fn resolve_project_status(error: &ResolveProjectError) -> Status {
    match error {
        ResolveProjectError::Unknown { .. } => Status::not_found(error.to_string()),
        ResolveProjectError::Unexpected { .. } => Status::internal(error.to_string()),
    }
}

fn task_location_status(error: &TaskLocationError) -> Status {
    Status::failed_precondition(error.to_string())
}

fn add_project_status(error: AddProjectError) -> Status {
    match error {
        AddProjectError::DuplicateProjectId { .. }
        | AddProjectError::DuplicateProjectTitle { .. } => {
            Status::already_exists(error.to_string())
        }
        AddProjectError::TaskLocation(error) => task_location_status(&error),
        AddProjectError::Unexpected { .. } => Status::internal(error.to_string()),
    }
}

fn get_project_status(error: &GetProjectError) -> Status {
    match error {
        GetProjectError::ProjectNotFound { .. } => Status::not_found(error.to_string()),
        GetProjectError::Unexpected { .. } => Status::internal(error.to_string()),
    }
}

fn list_projects_status(error: &ListProjectsError) -> Status {
    Status::internal(error.to_string())
}

fn pause_project_status(error: &PauseProjectError) -> Status {
    match error {
        PauseProjectError::ProjectNotFound { .. } => Status::not_found(error.to_string()),
        PauseProjectError::Unexpected { .. } => Status::internal(error.to_string()),
    }
}

fn rename_project_status(error: RenameProjectError) -> Status {
    match error {
        RenameProjectError::SourceProjectNotFound { .. } => Status::not_found(error.to_string()),
        RenameProjectError::SourceProjectChanged { .. } => Status::aborted(error.to_string()),
        RenameProjectError::DestinationProjectIdExists { .. }
        | RenameProjectError::DestinationProjectTitleExists { .. } => {
            Status::already_exists(error.to_string())
        }
        RenameProjectError::TaskLocation(error) => task_location_status(&error),
        RenameProjectError::Unexpected { .. }
        | RenameProjectError::StageTaskFiles { .. }
        | RenameProjectError::DiscardTaskFiles { .. }
        | RenameProjectError::BackupRetained { .. }
        | RenameProjectError::TaskFilesCommitRolledBack { .. }
        | RenameProjectError::TaskFilesCommitRollbackFailed { .. } => {
            Status::internal(error.to_string())
        }
    }
}

fn resume_project_status(error: ResumeProjectError) -> Status {
    match error {
        ResumeProjectError::ProjectNotFound { .. } => Status::not_found(error.to_string()),
        ResumeProjectError::TaskLocation(error) => task_location_status(&error),
        ResumeProjectError::Unexpected { .. } => Status::internal(error.to_string()),
    }
}
