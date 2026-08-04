use pwf_application::task::{AddTaskOk, add_task::AddTaskError};

pub(in crate::task) const TITLE_NORMALIZED_NOTICE: &str =
    "info: title normalized to keep metadata valid";

pub(in crate::task) fn emit_created_section(task: &AddTaskOk) {
    if let Some(section) = task.created_section.as_deref() {
        eprintln!("info: created `## {section}` section in {}", task.project);
    }
}

pub(in crate::task) fn emit_created_section_for_error(error: &AddTaskError) {
    if let Some((project, section)) = created_section_for_error(error) {
        eprintln!("info: created `## {section}` section in {project}");
    }
}

fn created_section_for_error(error: &AddTaskError) -> Option<(&str, &str)> {
    match error {
        AddTaskError::WriteStore { diagnostics, .. } => diagnostics
            .created_section
            .as_deref()
            .map(|section| (diagnostics.project.as_str(), section)),
        AddTaskError::Usage
        | AddTaskError::InvalidSection { .. }
        | AddTaskError::ProjectResolution(_)
        | AddTaskError::QueryProject(_)
        | AddTaskError::InvalidTag { .. }
        | AddTaskError::UnknownPrerequisiteIds { .. }
        | AddTaskError::InvalidTitle(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use pwf_application::task::add_task::{AddTaskDiagnostics, CreateTaskError};
    use pwf_infra::obsidian::ObsidianStoreError;
    use pwf_models::task::ProjectName;

    use super::*;

    #[test]
    fn add_write_index_error_exposes_created_section_diagnostic_data() {
        let error = AddTaskError::WriteStore {
            diagnostics: AddTaskDiagnostics {
                project: "foo-bar".to_string(),
                created_section: Some("Human".to_string()),
            },
            source: CreateTaskError::InsertIndex {
                project: ProjectName::try_new("foo-bar").unwrap(),
                created_section: Some("Human".to_string()),
                source: Box::new(ObsidianStoreError::AddWriteIndexFile {
                    source: std::io::Error::other("index write failed"),
                    project: "foo-bar".to_string(),
                    created_section: Some("Human".to_string()),
                }),
            },
        };

        assert_eq!(
            created_section_for_error(&error),
            Some(("foo-bar", "Human"))
        );
    }
}
