//! Shares private handoff lifecycle policy across pending-work mutations.

mod commit;
mod preflight;
#[cfg(test)]
mod tests;

use std::path::PathBuf;

pub(crate) use commit::{commit_after_pending_work, commit_scaffold, pending_handoff_path};
pub(crate) use preflight::{
    newest_handoff, preflight_close, preflight_delete, preflight_reopen, preflight_scaffold,
};
use pwf_domain::handoff::HandoffStatus;

use crate::{
    HandoffDocument, HandoffDocumentIdentifier, HandoffLocation, HandoffPatch, HandoffScope,
    NewHandoffDocument,
};

#[derive(Debug)]
pub(crate) struct PendingScaffold {
    scope: HandoffScope,
    document: NewHandoffDocument,
    identifier: HandoffDocumentIdentifier,
    path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CloseHandoffAction {
    Done,
    Cancelled,
}

impl CloseHandoffAction {
    fn status(self) -> HandoffStatus {
        match self {
            Self::Done => HandoffStatus::Done,
            Self::Cancelled => HandoffStatus::Cancelled,
        }
    }
}

#[derive(Debug)]
pub(crate) enum PendingHandoffMutation {
    NotLinked,
    AlreadyInTargetState,
    Create(PendingScaffold),
    Move(PendingMove),
    Delete(PendingDelete),
}

#[derive(Debug)]
pub(crate) struct PendingMove {
    scope: HandoffScope,
    snapshot: HandoffDocument,
    patch: HandoffPatch,
    outcome: MoveOutcome,
}

#[derive(Debug, Clone, Copy)]
enum MoveOutcome {
    Archived,
    Reopened,
}

#[derive(Debug)]
pub(crate) struct PendingDelete {
    scope: HandoffScope,
    snapshot: HandoffDocument,
}

pub(crate) fn scope(repository_root: impl Into<PathBuf>) -> HandoffScope {
    HandoffScope {
        repository_root: repository_root.into(),
    }
}

fn handoff_path(scope: &HandoffScope, identifier: &HandoffDocumentIdentifier) -> PathBuf {
    handoff_directory(scope, identifier.location).join(&identifier.file_name)
}

fn handoff_directory(scope: &HandoffScope, location: HandoffLocation) -> PathBuf {
    let directory = scope.repository_root.join("docs").join("handoffs");
    match location {
        HandoffLocation::Active => directory,
        HandoffLocation::Archived => directory.join("archived"),
    }
}

pub(crate) fn handoff_body(title: &str) -> String {
    format!(
        "\n# {title}\n\n## Goals\n- [ ] <task title> :: <task description>\n\n## Context\n\n## Next steps\n-\n\n<!-- Lifecycle: while active, this is a LIVE document — check off Goals as you finish them.\n     When all Goals are done run `pwf done --id <pw-id>` (closes the task and archives this handoff). Never edit archived/. -->\n"
    )
}
