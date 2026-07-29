use crate::{
    AppRecordStore, NoteMarkdownSource, PendingWorkRecord,
    pending_work::{
        project_registry::ProjectRegistry,
        show_pending_work_item::{
            self, ShowOutput, ShowPendingWorkError, ShowPendingWorkItem, ShowPendingWorkItemOk,
        },
    },
};

pub(super) fn load(
    id: &str,
    store: &impl AppRecordStore<PendingWorkRecord>,
    projects: &ProjectRegistry,
    markdown_source: &impl NoteMarkdownSource,
) -> Result<String, ShowPendingWorkError> {
    let output = show_pending_work_item::execute(
        &ShowPendingWorkItem {
            id: id.to_string(),
            output: ShowOutput::Markdown,
        },
        store,
        projects,
        markdown_source,
    )?;
    let ShowPendingWorkItemOk::Markdown(markdown) = output else {
        unreachable!("Markdown request returned a different representation")
    };
    Ok(markdown)
}
