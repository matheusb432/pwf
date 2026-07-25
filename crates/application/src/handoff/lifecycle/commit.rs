use std::path::Path;

use pwf_domain::pending_work::WorkItemId;

use super::{
    MoveOutcome, PendingDelete, PendingHandoffMutation, PendingMove, PendingScaffold,
    handoff_directory, handoff_path,
};
use crate::{
    AppRecordStore, HandoffDocument, HandoffDocumentIdentifier, HandoffDocumentStore,
    HandoffLedger, HandoffLocation, HandoffScope,
    handoff::{HandoffError, HandoffMutationOk, ledger},
};

pub(crate) fn commit_scaffold<S>(
    store: &S,
    mut pending: PendingScaffold,
    pending_work_identifier: &WorkItemId,
) -> Result<HandoffMutationOk, HandoffError>
where
    S: HandoffDocumentStore + AppRecordStore<HandoffLedger>,
{
    pending.document.pending_work_identifier = Some(pending_work_identifier.clone());
    commit_after_pending_work(store, PendingHandoffMutation::Create(pending))
}

pub(crate) fn pending_handoff_path(mutation: &PendingHandoffMutation) -> Option<&Path> {
    match mutation {
        PendingHandoffMutation::Move(pending) => Some(pending.snapshot.locator.as_path()),
        PendingHandoffMutation::Delete(pending) => Some(pending.snapshot.locator.as_path()),
        PendingHandoffMutation::Create(pending) => Some(pending.path.as_path()),
        PendingHandoffMutation::NotLinked | PendingHandoffMutation::AlreadyInTargetState => None,
    }
}

#[rustfmt::skip]
pub(crate) fn commit_after_pending_work<S>(
    store: &S,
    pending: PendingHandoffMutation,
) -> Result<HandoffMutationOk, HandoffError>
where
    S: HandoffDocumentStore + AppRecordStore<HandoffLedger>,
{
    // FIXME: Add a durable mutation journal before claiming atomic pending-work and handoff updates; process termination between stores can leave only the first record committed.
    match pending {
        PendingHandoffMutation::NotLinked => Ok(HandoffMutationOk::NotLinked),
        PendingHandoffMutation::AlreadyInTargetState => {
            Ok(HandoffMutationOk::AlreadyInTargetState)
        }
        PendingHandoffMutation::Create(pending) => commit_create(store, pending),
        PendingHandoffMutation::Move(pending) => commit_move(store, pending),
        PendingHandoffMutation::Delete(pending) => commit_delete(store, &pending),
    }
}

fn commit_create<S>(store: &S, pending: PendingScaffold) -> Result<HandoffMutationOk, HandoffError>
where
    S: HandoffDocumentStore + AppRecordStore<HandoffLedger>,
{
    let document =
        <S as AppRecordStore<HandoffDocument>>::insert(store, &pending.scope, pending.document)
            .map_err(|source| HandoffError::WriteDocument {
                operation: "create",
                path: pending.path.clone(),
                source: Box::new(source),
            })?;
    if let Err(source) = ledger::rebuild(&pending.scope, store) {
        let _ = <S as AppRecordStore<HandoffDocument>>::delete(
            store,
            &pending.scope,
            &pending.identifier,
        );
        return Err(map_ledger_error(&pending.scope, source));
    }
    Ok(HandoffMutationOk::Created {
        path: document.locator,
    })
}

fn commit_move<S>(store: &S, pending: PendingMove) -> Result<HandoffMutationOk, HandoffError>
where
    S: HandoffDocumentStore + AppRecordStore<HandoffLedger>,
{
    let destination_location = pending
        .patch
        .location
        .expect("a pending handoff move has a destination");
    let destination = HandoffDocumentIdentifier {
        file_name: pending.snapshot.identifier.file_name.clone(),
        location: destination_location,
    };
    let destination_path = handoff_path(&pending.scope, &destination);
    <S as AppRecordStore<HandoffDocument>>::update(
        store,
        &pending.scope,
        &pending.snapshot.identifier,
        pending.patch,
    )
    .map_err(|source| HandoffError::WriteDocument {
        operation: match pending.outcome {
            MoveOutcome::Archived => "archive",
            MoveOutcome::Reopened => "reopen",
        },
        path: destination_path.clone(),
        source: Box::new(source),
    })?;
    if let Err(source) = ledger::rebuild(&pending.scope, store) {
        let _ = store.restore_document_after_move(&pending.scope, &pending.snapshot);
        return Err(map_ledger_error(&pending.scope, source));
    }
    Ok(match pending.outcome {
        MoveOutcome::Archived => HandoffMutationOk::Archived {
            path: destination_path,
        },
        MoveOutcome::Reopened => HandoffMutationOk::Reopened {
            path: destination_path,
        },
    })
}

fn commit_delete<S>(store: &S, pending: &PendingDelete) -> Result<HandoffMutationOk, HandoffError>
where
    S: HandoffDocumentStore + AppRecordStore<HandoffLedger>,
{
    let path = pending.snapshot.locator.clone();
    <S as AppRecordStore<HandoffDocument>>::delete(
        store,
        &pending.scope,
        &pending.snapshot.identifier,
    )
    .map_err(|source| HandoffError::DeleteDocument {
        path: path.clone(),
        source: Box::new(source),
    })?;
    if let Err(source) = ledger::rebuild(&pending.scope, store) {
        let _ = store.restore_document_after_delete(&pending.scope, &pending.snapshot);
        return Err(map_ledger_error(&pending.scope, source));
    }
    Ok(HandoffMutationOk::Removed { path })
}

fn map_ledger_error(scope: &HandoffScope, source: ledger::RebuildLedgerError) -> HandoffError {
    HandoffError::RebuildLedger {
        path: handoff_directory(scope, HandoffLocation::Active).join("LEDGER.md"),
        source: Box::new(source),
    }
}
