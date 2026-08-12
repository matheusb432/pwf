use pwf_application::task::add_task::AddTaskError;
use pwf_wire::task::AddedTask;

pub(in crate::task) const TITLE_NORMALIZED_NOTICE: &str =
    "info: title normalized to keep metadata valid";

pub(in crate::task) fn emit_created_section(task: &AddedTask) {
    if let Some(section) = task.created_section.as_ref() {
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
            .as_ref()
            .map(|section| (diagnostics.project.as_ref(), section.as_ref())),
        AddTaskError::ProjectResolution(_)
        | AddTaskError::QueryProject(_)
        | AddTaskError::UnknownBlockedByIds { .. }
        | AddTaskError::InvalidTitle(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use pwf_application::task::add_task::CreateTaskError;
    use pwf_infra::obsidian::ObsidianStoreError;
    use pwf_models::{project::ProjectName, task::TaskSection};
    use pwf_wire::task::AddTaskDiagnostics;

    use super::*;

    #[test]
    fn add_write_index_error_exposes_created_section_diagnostic_data() {
        let error = AddTaskError::WriteStore {
            diagnostics: AddTaskDiagnostics {
                project: ProjectName::try_new("foo-bar").unwrap(),
                created_section: Some(TaskSection::human()),
            },
            source: CreateTaskError::InsertIndex {
                project: ProjectName::try_new("foo-bar").unwrap(),
                created_section: Some(TaskSection::human()),
                source: Box::new(ObsidianStoreError::AddWriteIndexFile {
                    source: std::io::Error::other("index write failed"),
                    project: "foo-bar".to_string(),
                    created_section: Some(TaskSection::human()),
                }),
            },
        };

        assert_eq!(
            created_section_for_error(&error),
            Some(("foo-bar", "Human"))
        );
    }
}
