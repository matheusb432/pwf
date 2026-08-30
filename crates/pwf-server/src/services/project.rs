use pwf_application::project::{
    TaskLocationError,
    add_project::{self, AddProjectError},
    get_project::{self, GetProjectError},
    list_projects::{self, ListProjectsError},
    pause_project::{self, PauseProjectError},
    rename_project::{self, RenameProjectError},
    resolve_project::ResolveProjectError,
    resume_project::{self, ResumeProjectError},
    update_project::{self, UpdateProjectError},
};
use pwf_infra::obsidian::ObsidianProjectTaskFilesClient;
use pwf_wire::{
    pb::{self, project_service_server::ProjectService},
    proto,
};
use tonic::{Request, Response, Status};

use crate::AppState;

pub(crate) struct ProjectGrpcService {
    state: AppState,
}

impl ProjectGrpcService {
    pub(crate) fn new(state: AppState) -> Self {
        Self { state }
    }
}

#[tonic::async_trait]
impl ProjectService for ProjectGrpcService {
    async fn add_project(
        &self,
        request: Request<pb::AddProjectRequest>,
    ) -> Result<Response<pb::AddProjectResponse>, Status> {
        let fields = proto::project::add_project_request(request.into_inner())?;
        add_project::execute(fields, &self.state.pool, &self.state.home)
            .await
            .map(proto::project::add_project_response)
            .map(Response::new)
            .map_err(add_project_status)
    }

    async fn get_project(
        &self,
        request: Request<pb::GetProjectRequest>,
    ) -> Result<Response<pb::GetProjectResponse>, Status> {
        let query = proto::project::get_project_request(request.into_inner())?;
        get_project::execute(query, &self.state.pool)
            .await
            .map(proto::project::get_project_response)
            .map(Response::new)
            .map_err(|error| get_project_status(&error))
    }

    async fn list_projects(
        &self,
        request: Request<pb::ListProjectsRequest>,
    ) -> Result<Response<pb::ListProjectsResponse>, Status> {
        let status = proto::project::list_projects_request(request.into_inner())?;
        list_projects::execute(status, &self.state.pool)
            .await
            .map(proto::project::list_projects_response)
            .map(Response::new)
            .map_err(|error| list_projects_status(&error))
    }

    async fn pause_project(
        &self,
        request: Request<pb::PauseProjectRequest>,
    ) -> Result<Response<pb::PauseProjectResponse>, Status> {
        let project_id = proto::project::pause_project_request(request.into_inner())?;
        pause_project::execute(project_id, &self.state.pool)
            .await
            .map(proto::project::pause_project_response)
            .map(Response::new)
            .map_err(|error| pause_project_status(&error))
    }

    async fn rename_project(
        &self,
        request: Request<pb::RenameProjectRequest>,
    ) -> Result<Response<pb::RenameProjectResponse>, Status> {
        let command = proto::project::rename_project_request(request.into_inner())?;
        rename_project::execute(
            command,
            &self.state.pool,
            &ObsidianProjectTaskFilesClient,
            &self.state.home,
        )
        .await
        .map(proto::project::rename_project_response)
        .map(Response::new)
        .map_err(rename_project_status)
    }

    async fn resume_project(
        &self,
        request: Request<pb::ResumeProjectRequest>,
    ) -> Result<Response<pb::ResumeProjectResponse>, Status> {
        let project_id = proto::project::resume_project_request(request.into_inner())?;
        resume_project::execute(project_id, &self.state.pool, &self.state.home)
            .await
            .map(proto::project::resume_project_response)
            .map(Response::new)
            .map_err(resume_project_status)
    }

    async fn update_project(
        &self,
        request: Request<pb::UpdateProjectRequest>,
    ) -> Result<Response<pb::UpdateProjectResponse>, Status> {
        let command = proto::project::update_project_request(request.into_inner())?;
        update_project::execute(command, &self.state.pool)
            .await
            .map(|()| proto::project::update_project_response())
            .map(Response::new)
            .map_err(|error| update_project_status(&error))
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

fn update_project_status(error: &UpdateProjectError) -> Status {
    match error {
        UpdateProjectError::ProjectNotFound { .. } => Status::not_found(error.to_string()),
        UpdateProjectError::Unexpected { .. } => Status::internal(error.to_string()),
    }
}
