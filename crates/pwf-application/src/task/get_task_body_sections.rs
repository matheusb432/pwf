use pwf_models::project::ProjectId;
use pwf_wire::{
    project::{GetProject, ProjectStatusFilter},
    task::{GetTaskBodySections, TaskBodyItemStyle, TaskBodySection, TaskBodySections},
};

use super::body_presets::{MarkerSectionItemStyle, TaskBodyPreset};
use crate::{
    ports::{
        project_store::ProjectStore,
        user_settings::{TaskBodyPresetReader, UserSettingsLoadError},
    },
    project::get_project::GetProjectError,
};

#[derive(Debug, thiserror::Error)]
pub enum GetTaskBodySectionsError {
    #[error("project not found: {id}")]
    ProjectNotFound { id: ProjectId },
    #[error(transparent)]
    Settings(#[from] UserSettingsLoadError),
    #[error("{context}: {source}")]
    QueryProject {
        context: &'static str,
        #[source]
        source: anyhow::Error,
    },
}

/// Describes the shorthand sections for a registered project, or the global selection.
#[cqrsy::query]
pub async fn execute(
    query: GetTaskBodySections,
    preset_reader: &impl TaskBodyPresetReader,
    project_store: &impl ProjectStore,
) -> Result<TaskBodySections, GetTaskBodySectionsError> {
    let presets = preset_reader.load_task_body_presets()?;
    let preset = match query.project_id {
        Some(project_id) => {
            let project = project_store
                .get_project(GetProject::new(
                    project_id,
                    ProjectStatusFilter::IncludingPaused,
                ))
                .await
                .map_err(|error| match error {
                    GetProjectError::ProjectNotFound { id } => {
                        GetTaskBodySectionsError::ProjectNotFound { id }
                    }
                    GetProjectError::Unexpected { context, source } => {
                        GetTaskBodySectionsError::QueryProject { context, source }
                    }
                })?;
            presets.for_project(&project.id)
        }
        None => presets.selected(),
    };
    Ok(task_body_sections(preset))
}

fn task_body_sections(preset: &TaskBodyPreset) -> TaskBodySections {
    TaskBodySections {
        preset: preset.name().to_string(),
        sections: preset
            .sections()
            .iter()
            .map(|section| TaskBodySection {
                marker: section.marker().to_string(),
                header: section.header().to_string(),
                heading_level: section.heading_level().get(),
                item_style: match section.item_style() {
                    MarkerSectionItemStyle::Bullet => TaskBodyItemStyle::Bullet,
                    MarkerSectionItemStyle::Numbered => TaskBodyItemStyle::Numbered,
                    MarkerSectionItemStyle::Paragraph => TaskBodyItemStyle::Paragraph,
                },
            })
            .collect(),
    }
}
