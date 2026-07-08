use pwf_application::AddPendingWorkError;
use pwf_infra::obsidian::ObsidianPendingWorkStoreError;

use super::outcome::AddedItem;

pub(crate) fn emit_created_section_diagnostic(item: &AddedItem) {
    if let Some(section) = item.created_section.as_deref() {
        eprintln!("info: created `## {section}` section in {}", item.project);
    }
}

pub(crate) fn emit_created_section_diagnostic_for_error(error: &AddPendingWorkError) {
    let AddPendingWorkError::WriteStore(source) = error;
    if let Some(store_error) = source
        .as_ref()
        .downcast_ref::<ObsidianPendingWorkStoreError>()
        && let Some((project, section)) = store_error.created_section_diagnostic()
    {
        eprintln!("info: created `## {section}` section in {project}");
    }
}
