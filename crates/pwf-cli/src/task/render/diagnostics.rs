use pwf_wire::task::{AddTaskApiError, AddedTask};

pub(in crate::task) const TITLE_NORMALIZED_NOTICE: &str =
    "info: title normalized to keep metadata valid";

pub(in crate::task) fn emit_created_section(task: &AddedTask) {
    if let Some(section) = task.created_section.as_ref() {
        eprintln!("info: created `## {section}` section in {}", task.project);
    }
}

pub(in crate::task) fn emit_created_section_for_error(error: &AddTaskApiError) {
    if let Some((project, section)) = created_section_for_error(error) {
        eprintln!("info: created `## {section}` section in {project}");
    }
}

fn created_section_for_error(error: &AddTaskApiError) -> Option<(&str, &str)> {
    match error {
        AddTaskApiError::WriteStore { diagnostics, .. } => diagnostics
            .created_section
            .as_ref()
            .map(|section| (diagnostics.project.as_ref(), section.as_ref())),
        AddTaskApiError::InvalidRequest
        | AddTaskApiError::UnsupportedTaskCreation
        | AddTaskApiError::Input(_)
        | AddTaskApiError::ResolveProject(_)
        | AddTaskApiError::UnknownBlockedByIds { .. }
        | AddTaskApiError::ReadBlockedBy { .. }
        | AddTaskApiError::Unexpected { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use pwf_models::{project::ProjectName, task::TaskSection};
    use pwf_wire::task::AddTaskDiagnostics;

    use super::*;

    #[test]
    fn add_write_index_error_exposes_created_section_diagnostic_data() {
        let error = AddTaskApiError::WriteStore {
            diagnostics: AddTaskDiagnostics {
                project: ProjectName::try_new("foo-bar").unwrap(),
                created_section: Some(TaskSection::human()),
            },
            message: "index write failed".to_string(),
        };

        assert_eq!(
            created_section_for_error(&error),
            Some(("foo-bar", "Human"))
        );
    }
}
