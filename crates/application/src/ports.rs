mod agent_client;
mod app_db_store;
mod app_record_store;
mod clock;
mod confirmation_client;
mod inline_agent_session_client;
mod pending_work_record_store;
mod project_note_store;
mod project_task_files_client;
mod project_task_location_client;
mod repository_directory_client;
mod session_client;

pub use agent_client::{AgentClient, PreparedAgentLaunch};
pub use app_db_store::AppDbStore;
#[cfg(test)]
pub(crate) use app_db_store::TestDatabase;
pub use app_record_store::{AppRecordStore, Record};
pub use clock::Clock;
pub use confirmation_client::{Confirmation, ConfirmationClient};
pub use inline_agent_session_client::InlineAgentSessionClient;
pub use pending_work_record_store::{
    IndexEntry, IndexEntryState, IndexPlacement, IndexSection, ItemPatch, Materialization, NewItem,
    PendingWorkRecord, RecordId,
};
pub use project_note_store::{NewProjectNote, ProjectNotePatch, ProjectNoteStore};
pub use project_task_files_client::{
    ProjectTaskFilesClient, ProjectTaskFilesRenameCommit, StagedProjectTaskFilesRename,
};
pub use project_task_location_client::ProjectTaskLocationClient;
pub use repository_directory_client::RepositoryDirectoryClient;
pub use session_client::{AgentCommand, SessionClient, SessionStart, SessionWindow};
