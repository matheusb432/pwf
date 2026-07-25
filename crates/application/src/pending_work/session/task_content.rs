use crate::{
    AppRecordStore, NoteMarkdownSource, PendingWorkItem,
    pending_work::{
        project_registry::ProjectRegistry,
        show_pending_work_item::{self, ShowOutput, ShowPendingWorkError, ShowPendingWorkItem},
    },
};

pub(super) fn load(
    id: &str,
    store: &impl AppRecordStore<PendingWorkItem>,
    projects: &ProjectRegistry,
    markdown_source: &impl NoteMarkdownSource,
) -> Result<String, ShowPendingWorkError> {
    show_pending_work_item::execute(
        &ShowPendingWorkItem {
            id: id.to_string(),
            output: ShowOutput::Markdown,
        },
        store,
        projects,
        markdown_source,
    )
}
