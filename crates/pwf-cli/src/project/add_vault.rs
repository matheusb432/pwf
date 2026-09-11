use std::path::{Path, PathBuf};

use anyhow::Context as _;
use clap::Args;
use pwf_client::{pb::AddVaultProjectRequest, project::ProjectClient};
use pwf_models::project::{ProjectId, ProjectName, ProjectTasksRelativePath};

use super::{output, parse_project_id, parse_project_title};

#[derive(Args, Debug)]
pub struct Arguments {
    /// Outputs the result as JSON.
    #[arg(long)]
    pub json: bool,
    /// vault root; defaults to the current directory.
    #[arg(value_name = "PATH", default_value = ".")]
    pub path: PathBuf,
    /// project id.
    #[arg(long, value_parser = parse_project_id)]
    pub id: ProjectId,
    /// task folder relative to the vault root.
    #[arg(long)]
    pub tasks_path: ProjectTasksRelativePath,
    /// project title; defaults to the task folder name.
    #[arg(long, value_parser = parse_project_title)]
    pub title: Option<ProjectName>,
    /// source directory for agent sessions; omitted means no source.
    #[arg(long)]
    pub source_path: Option<PathBuf>,
}

pub(super) async fn run(
    arguments: Arguments,
    console: crate::console::Console,
    colors: pwf_models::settings::ProjectStatusColors,
    client: &ProjectClient,
) -> anyhow::Result<String> {
    let response = client
        .add_vault_project(AddVaultProjectRequest {
            vault_path: request_path(&arguments.path)?,
            id: arguments.id.to_string(),
            tasks_path: arguments.tasks_path.to_string(),
            title: arguments.title.map(|title| title.to_string()),
            source_path: arguments
                .source_path
                .as_deref()
                .map(request_path)
                .transpose()?,
        })
        .await
        .map_err(crate::rpc_error)?;
    if arguments.json {
        serde_json::to_string_pretty(&serde_json::json!({ "id": response.id })).map_err(Into::into)
    } else {
        let project = client
            .get_project(pwf_client::pb::GetProjectRequest {
                id: response.id,
                status: pwf_client::pb::ProjectStatusFilter::IncludingPaused as i32,
            })
            .await
            .map_err(crate::rpc_error)?;
        Ok(output::render_mutation(
            output::ProjectMutationAction::Added,
            project,
            colors,
            console.color(),
        ))
    }
}

fn request_path(path: &Path) -> anyhow::Result<String> {
    let raw = path.to_str().context("path is not valid unicode")?;
    anyhow::ensure!(!raw.trim().is_empty(), "path must not be blank");
    if raw == "~" || raw.starts_with("~/") || raw.starts_with("~\\") {
        return Ok(raw.to_string());
    }
    std::path::absolute(path)?
        .into_os_string()
        .into_string()
        .map_err(|_| anyhow::anyhow!("path is not valid unicode"))
}
