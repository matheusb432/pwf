pub mod handoff;
pub mod note;
pub mod pending_work;
pub mod ports;
pub mod project;

pub use ports::{
    AppDbStore, AppRecordStore, Clock, HandoffDocument, HandoffDocumentIdentifier,
    HandoffDocumentScopePresence, HandoffDocumentStore, HandoffLedger, HandoffLedgerIdentifier,
    HandoffLedgerRow, HandoffLedgerWrite, HandoffLocation, HandoffPatch, HandoffScope, IndexEntry,
    IndexEntryState, IndexPlacement, IndexSection, ItemPatch, Materialization, NewHandoffDocument,
    NewItem, NewProjectNote, NoteMarkdownSource, PendingWorkItem, ProjectNote, ProjectNotePatch,
    ProjectNoteStore, Record, RecordId,
};

#[cfg(test)]
mod testing;
