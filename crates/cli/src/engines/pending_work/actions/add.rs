use pwf_application::pending_work::add::AddPendingWorkError;
use pwf_infra::obsidian::ObsidianStoreError;

use super::outcome::AddedItem;

/// Stderr notice shared by `add` and `update` when a title needed YAML-safety rewriting.
pub(in crate::engines::pending_work) const TITLE_NORMALIZED_NOTICE: &str =
    "info: title normalized to keep metadata valid";

pub(crate) fn emit_created_section_diagnostic(item: &AddedItem) {
    if let Some(section) = item.created_section.as_deref() {
        eprintln!("info: created `## {section}` section in {}", item.project);
    }
}

pub(crate) fn emit_created_section_diagnostic_for_error(error: &AddPendingWorkError) {
    if let Some((project, section)) = created_section_diagnostic_for_error(error) {
        eprintln!("info: created `## {section}` section in {project}");
    }
}

pub(super) fn created_section_diagnostic_for_error(
    error: &AddPendingWorkError,
) -> Option<(&str, &str)> {
    match error {
        AddPendingWorkError::WriteStore(source) => source
            .as_ref()
            .downcast_ref::<ObsidianStoreError>()
            .and_then(ObsidianStoreError::created_section_diagnostic),
        AddPendingWorkError::ProjectNotMappedToRepo { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_write_index_error_exposes_created_section_diagnostic_data() {
        let error =
            AddPendingWorkError::WriteStore(Box::new(ObsidianStoreError::AddWriteIndexFile {
                source: std::io::Error::other("index write failed"),
                project: "glep-shimeji".to_string(),
                created_section: Some("Human".to_string()),
            }));

        assert_eq!(
            created_section_diagnostic_for_error(&error),
            Some(("glep-shimeji", "Human"))
        );
    }
}
