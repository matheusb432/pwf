use pwf_application::pending_work::{
    AddPendingWorkItemOk, add_pending_work_item::AddPendingWorkError,
};

pub(in crate::pending_work) const TITLE_NORMALIZED_NOTICE: &str =
    "info: title normalized to keep metadata valid";

pub(in crate::pending_work) fn emit_created_section(item: &AddPendingWorkItemOk) {
    if let Some(section) = item.created_section.as_deref() {
        eprintln!("info: created `## {section}` section in {}", item.project);
    }
}

pub(in crate::pending_work) fn emit_created_section_for_error(error: &AddPendingWorkError) {
    if let Some((project, section)) = created_section_for_error(error) {
        eprintln!("info: created `## {section}` section in {project}");
    }
}

fn created_section_for_error(error: &AddPendingWorkError) -> Option<(&str, &str)> {
    match error {
        AddPendingWorkError::WriteStore { diagnostics, .. } => diagnostics
            .created_section
            .as_deref()
            .map(|section| (diagnostics.project.as_str(), section)),
        AddPendingWorkError::ProjectHasNoDirectorySource { .. }
        | AddPendingWorkError::Usage
        | AddPendingWorkError::InvalidSection { .. }
        | AddPendingWorkError::ProjectResolution(_)
        | AddPendingWorkError::InvalidTag { .. }
        | AddPendingWorkError::InvalidPrerequisiteId { .. }
        | AddPendingWorkError::MissingPrerequisiteId
        | AddPendingWorkError::UnknownPrerequisiteIds { .. }
        | AddPendingWorkError::InvalidTitle(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use pwf_application::pending_work::add_pending_work_item::{
        AddPendingWorkDiagnostics, CreateItemError,
    };
    use pwf_infra::obsidian::ObsidianStoreError;
    use pwf_models::pending_work::ProjectName;

    use super::*;

    #[test]
    fn add_write_index_error_exposes_created_section_diagnostic_data() {
        let error = AddPendingWorkError::WriteStore {
            diagnostics: AddPendingWorkDiagnostics {
                project: "foo-bar".to_string(),
                created_section: Some("Human".to_string()),
            },
            source: CreateItemError::InsertIndex {
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
