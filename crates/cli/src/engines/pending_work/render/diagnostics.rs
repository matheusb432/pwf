use pwf_application::pending_work::{
    AddedItem,
    add::{AddPendingWorkDiagnostics, AddPendingWorkError},
};

pub(in crate::engines::pending_work) const TITLE_NORMALIZED_NOTICE: &str =
    "info: title normalized to keep metadata valid";

pub(in crate::engines::pending_work) const ADD_MIRROR_REMEDY: &str = "the item was created but its handoff scaffold failed; create the handoff manually or remove the `handoff` tag";

pub(in crate::engines::pending_work) const REMOVE_MIRROR_REMEDY: &str =
    "the pw note is already deleted; delete the linked handoff file by hand";

pub(in crate::engines::pending_work) fn done_cancel_reopen_remedy(id: &str) -> String {
    format!(
        "fix the cause, then `pwf reopen --id {id}` and re-run — or finish the handoff move by hand"
    )
}

pub(in crate::engines::pending_work) fn emit_created_section(item: &AddedItem) {
    if let Some(section) = item.created_section.as_deref() {
        eprintln!("info: created `## {section}` section in {}", item.project);
    }
}

pub(in crate::engines::pending_work) fn emit_add_diagnostics(
    diagnostics: &AddPendingWorkDiagnostics,
) {
    if let Some(section) = diagnostics.created_section.as_deref() {
        eprintln!(
            "info: created `## {section}` section in {}",
            diagnostics.project
        );
    }
    if diagnostics.title_normalized {
        eprintln!("{TITLE_NORMALIZED_NOTICE}");
    }
}

pub(in crate::engines::pending_work) fn emit_created_section_for_error(
    error: &AddPendingWorkError,
) {
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
        AddPendingWorkError::ProjectNotMappedToRepo { .. }
        | AddPendingWorkError::Usage
        | AddPendingWorkError::ProjectResolution(_)
        | AddPendingWorkError::InvalidTag { .. }
        | AddPendingWorkError::InvalidPrerequisiteId { .. }
        | AddPendingWorkError::MissingPrerequisiteId
        | AddPendingWorkError::UnknownPrerequisiteIds { .. }
        | AddPendingWorkError::HandoffPreflight(_)
        | AddPendingWorkError::HandoffAfterPendingWork { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use pwf_domain::pending_work::ProjectName;
    use pwf_infra::obsidian::ObsidianStoreError;

    use super::*;

    #[test]
    fn add_write_index_error_exposes_created_section_diagnostic_data() {
        let error = AddPendingWorkError::WriteStore {
            diagnostics: AddPendingWorkDiagnostics {
                project: "glep-shimeji".to_string(),
                created_section: Some("Human".to_string()),
                title_normalized: false,
            },
            source: pwf_application::pending_work::store_util::CreateItemError::InsertIndex {
                project: ProjectName::try_new("glep-shimeji").unwrap(),
                created_section: Some("Human".to_string()),
                source: Box::new(ObsidianStoreError::AddWriteIndexFile {
                    source: std::io::Error::other("index write failed"),
                    project: "glep-shimeji".to_string(),
                    created_section: Some("Human".to_string()),
                }),
            },
        };

        assert_eq!(
            created_section_for_error(&error),
            Some(("glep-shimeji", "Human"))
        );
    }
}
