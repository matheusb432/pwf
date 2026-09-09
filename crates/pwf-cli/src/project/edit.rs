use clap::Args;
use pwf_client::{
    pb::{StringPatchField, UpdateProjectRequest, string_patch_field},
    project::ProjectClient,
};
use pwf_models::project::{ProjectId, ProjectSourceValue};

use super::{parse_project_id, parse_project_source};

#[derive(Args, Debug)]
#[command(group(clap::ArgGroup::new("updates").args(["source", "clear_source", "obsidian_vault", "clear_obsidian_vault"]).required(true).multiple(true)))]
pub struct Arguments {
    /// Project ID.
    #[arg(value_parser = parse_project_id)]
    pub id: ProjectId,
    /// Replacement project source directory.
    #[arg(long, value_parser = parse_project_source, conflicts_with = "clear_source")]
    pub source: Option<ProjectSourceValue>,
    /// remove the source directory; sessions will be unavailable.
    #[arg(long)]
    pub clear_source: bool,
    /// Obsidian vault root whose existing .trash receives removed tasks.
    #[arg(long, conflicts_with = "clear_obsidian_vault")]
    pub obsidian_vault: Option<pwf_models::project::ObsidianVault>,
    /// Permanently delete removed tasks instead of moving them to an Obsidian trash folder.
    #[arg(long)]
    pub clear_obsidian_vault: bool,
}

pub(super) async fn run(arguments: Arguments, client: &ProjectClient) -> anyhow::Result<String> {
    client
        .update_project(UpdateProjectRequest {
            id: arguments.id.to_string(),
            source_value: arguments
                .source
                .map(|value| StringPatchField {
                    operation: Some(string_patch_field::Operation::Set(value.to_string())),
                })
                .or_else(|| {
                    arguments.clear_source.then_some(StringPatchField {
                        operation: Some(string_patch_field::Operation::Clear(
                            pwf_client::pb::ClearField {},
                        )),
                    })
                }),
            obsidian_vault: arguments
                .obsidian_vault
                .map(|value| StringPatchField {
                    operation: Some(string_patch_field::Operation::Set(value.to_string())),
                })
                .or_else(|| {
                    arguments.clear_obsidian_vault.then_some(StringPatchField {
                        operation: Some(string_patch_field::Operation::Clear(
                            pwf_client::pb::ClearField {},
                        )),
                    })
                }),
        })
        .await
        .map_err(crate::rpc_error)?;
    Ok(String::new())
}
