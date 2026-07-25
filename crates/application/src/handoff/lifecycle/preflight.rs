use std::path::Path;

use pwf_domain::{
    handoff::HandoffStatus,
    pending_work::{ProjectName, Timestamp},
};

use super::{
    CloseHandoffAction, MoveOutcome, PendingDelete, PendingHandoffMutation, PendingMove,
    PendingScaffold, handoff_body, handoff_directory, handoff_path, scope,
};
use crate::{
    AppRecordStore, HandoffDocument, HandoffDocumentIdentifier, HandoffDocumentScopePresence,
    HandoffDocumentStore, HandoffLocation, HandoffPatch, NewHandoffDocument, PendingWorkItem,
    handoff::{HandoffError, naming},
    pending_work::{ProjectRegistry, identifier, tag_policy},
};

struct GateItem {
    pending_work_identifier: String,
    scope: crate::HandoffScope,
}

pub(crate) fn preflight_scaffold<S>(
    store: &S,
    repository_root: &str,
    project: &ProjectName,
    title: &str,
    created: &str,
) -> Result<PendingScaffold, HandoffError>
where
    S: HandoffDocumentStore,
{
    let scope = scope(repository_root);
    if store
        .scope_presence(&scope)
        .map_err(|source| HandoffError::ReadDocuments {
            path: scope.repository_root.clone(),
            source: Box::new(source),
        })?
        == HandoffDocumentScopePresence::RepositoryMissing
    {
        return Err(HandoffError::RepositoryMissing {
            id: project.to_string(),
            path: scope.repository_root,
        });
    }
    let file_name = handoff_file_name(created, title);
    let identifier = HandoffDocumentIdentifier {
        file_name: file_name.clone(),
        location: HandoffLocation::Active,
    };
    let path = handoff_path(&scope, &identifier);
    if store
        .document_exists(&scope, &identifier)
        .map_err(|source| HandoffError::ReadDocuments {
            path: path.clone(),
            source: Box::new(source),
        })?
    {
        return Err(HandoffError::ActiveDestinationExists { path });
    }

    Ok(PendingScaffold {
        scope,
        document: NewHandoffDocument {
            file_name,
            project: project.clone(),
            title: title.to_string(),
            created: Timestamp::new(created),
            body: handoff_body(title),
            pending_work_identifier: None,
        },
        identifier,
        path,
    })
}

pub(crate) fn preflight_close<S>(
    store: &S,
    projects: &ProjectRegistry,
    pending_work_identifier: &str,
    action: CloseHandoffAction,
    completed: &str,
    report: Option<&str>,
) -> Result<PendingHandoffMutation, HandoffError>
where
    S: HandoffDocumentStore + AppRecordStore<PendingWorkItem>,
{
    let Some(gate) = handoff_gate(store, projects, pending_work_identifier)? else {
        return Ok(PendingHandoffMutation::NotLinked);
    };
    let active_directory = handoff_directory(&gate.scope, HandoffLocation::Active);
    let active_documents = documents_location(store, &gate, HandoffLocation::Active)?;
    let active = find_linked(
        &active_documents,
        &gate.pending_work_identifier,
        HandoffLocation::Active,
        |document| document.status == Some(HandoffStatus::Active),
        &active_directory,
    )?;
    let Some(snapshot) = active else {
        let archived_directory = handoff_directory(&gate.scope, HandoffLocation::Archived);
        let archived_documents = documents_location(store, &gate, HandoffLocation::Archived)?;
        if find_linked(
            &archived_documents,
            &gate.pending_work_identifier,
            HandoffLocation::Archived,
            |_| true,
            &archived_directory,
        )?
        .is_some()
        {
            return Ok(PendingHandoffMutation::AlreadyInTargetState);
        }
        return Err(HandoffError::HandoffNotFound {
            id: gate.pending_work_identifier,
            directory: active_directory,
        });
    };
    let destination = HandoffDocumentIdentifier {
        file_name: snapshot.identifier.file_name.clone(),
        location: HandoffLocation::Archived,
    };
    let destination_path = handoff_path(&gate.scope, &destination);
    if store
        .document_exists(&gate.scope, &destination)
        .map_err(|source| HandoffError::ReadDocuments {
            path: destination_path.clone(),
            source: Box::new(source),
        })?
    {
        return Err(HandoffError::ArchivedDestinationExists {
            path: destination_path,
        });
    }
    let body = (action == CloseHandoffAction::Cancelled)
        .then(|| cancelled_body(&snapshot.body, report.unwrap_or_default()));
    Ok(PendingHandoffMutation::Move(PendingMove {
        scope: gate.scope,
        snapshot: snapshot.clone(),
        patch: HandoffPatch {
            location: Some(HandoffLocation::Archived),
            status: Some(action.status()),
            completed: Some(Some(Timestamp::new(completed))),
            body,
            ..HandoffPatch::default()
        },
        outcome: MoveOutcome::Archived,
    }))
}

pub(crate) fn preflight_reopen<S>(
    store: &S,
    projects: &ProjectRegistry,
    pending_work_identifier: &str,
) -> Result<PendingHandoffMutation, HandoffError>
where
    S: HandoffDocumentStore + AppRecordStore<PendingWorkItem>,
{
    let Some(gate) = handoff_gate(store, projects, pending_work_identifier)? else {
        return Ok(PendingHandoffMutation::NotLinked);
    };
    let archived_directory = handoff_directory(&gate.scope, HandoffLocation::Archived);
    let archived_documents = documents_location(store, &gate, HandoffLocation::Archived)?;
    let archived = find_linked(
        &archived_documents,
        &gate.pending_work_identifier,
        HandoffLocation::Archived,
        |document| document.status != Some(HandoffStatus::Active),
        &archived_directory,
    )?;
    let Some(snapshot) = archived else {
        let active_directory = handoff_directory(&gate.scope, HandoffLocation::Active);
        let active_documents = documents_location(store, &gate, HandoffLocation::Active)?;
        if find_linked(
            &active_documents,
            &gate.pending_work_identifier,
            HandoffLocation::Active,
            |document| document.status == Some(HandoffStatus::Active),
            &active_directory,
        )?
        .is_some()
        {
            return Ok(PendingHandoffMutation::AlreadyInTargetState);
        }
        return Err(HandoffError::HandoffNotFound {
            id: gate.pending_work_identifier,
            directory: archived_directory,
        });
    };
    let destination = HandoffDocumentIdentifier {
        file_name: snapshot.identifier.file_name.clone(),
        location: HandoffLocation::Active,
    };
    let destination_path = handoff_path(&gate.scope, &destination);
    if store
        .document_exists(&gate.scope, &destination)
        .map_err(|source| HandoffError::ReadDocuments {
            path: destination_path.clone(),
            source: Box::new(source),
        })?
    {
        return Err(HandoffError::ActiveDestinationExists {
            path: destination_path,
        });
    }
    Ok(PendingHandoffMutation::Move(PendingMove {
        scope: gate.scope,
        snapshot: snapshot.clone(),
        patch: HandoffPatch {
            location: Some(HandoffLocation::Active),
            status: Some(HandoffStatus::Active),
            completed: Some(None),
            ..HandoffPatch::default()
        },
        outcome: MoveOutcome::Reopened,
    }))
}

pub(crate) fn preflight_delete<S>(
    store: &S,
    projects: &ProjectRegistry,
    pending_work_identifier: &str,
) -> Result<PendingHandoffMutation, HandoffError>
where
    S: HandoffDocumentStore + AppRecordStore<PendingWorkItem>,
{
    let Some(gate) = handoff_gate(store, projects, pending_work_identifier)? else {
        return Ok(PendingHandoffMutation::NotLinked);
    };
    let active_directory = handoff_directory(&gate.scope, HandoffLocation::Active);
    let documents = documents_location(store, &gate, HandoffLocation::Active)?;
    let snapshot = find_linked(
        &documents,
        &gate.pending_work_identifier,
        HandoffLocation::Active,
        |document| document.status == Some(HandoffStatus::Active),
        &active_directory,
    )?
    .ok_or_else(|| HandoffError::HandoffNotFound {
        id: gate.pending_work_identifier,
        directory: active_directory,
    })?;
    Ok(PendingHandoffMutation::Delete(PendingDelete {
        scope: gate.scope,
        snapshot: snapshot.clone(),
    }))
}

fn handoff_gate<S>(
    store: &S,
    projects: &ProjectRegistry,
    pending_work_identifier: &str,
) -> Result<Option<GateItem>, HandoffError>
where
    S: HandoffDocumentStore + AppRecordStore<PendingWorkItem>,
{
    let Some(identifier) = identifier::parse(pending_work_identifier) else {
        return Ok(None);
    };
    let Some(project) = projects.project_for_id(&identifier) else {
        return Ok(None);
    };
    let Some(item) = <S as AppRecordStore<PendingWorkItem>>::get(store, project, &identifier)
        .map_err(|source| HandoffError::ReadPendingWork {
            source: Box::new(source),
        })?
    else {
        return Ok(None);
    };
    let item_identifier = identifier.to_string();
    let Some(raw) = item.tags else {
        return Ok(None);
    };
    let tags = tag_policy::parse_frontmatter(&raw).map_err(|_| HandoffError::InvalidTags {
        id: item_identifier.clone(),
        raw,
    })?;
    if !tag_policy::contains_name(&tags, tag_policy::HANDOFF_TAG) {
        return Ok(None);
    }
    let repository = projects
        .repo_for(project)
        .filter(|repository| !repository.trim().is_empty())
        .ok_or_else(|| HandoffError::UnmanagedProject {
            id: item_identifier.clone(),
            project: project.to_string(),
        })?;
    let scope = scope(repository);
    if store
        .scope_presence(&scope)
        .map_err(|source| HandoffError::ReadDocuments {
            path: scope.repository_root.clone(),
            source: Box::new(source),
        })?
        == HandoffDocumentScopePresence::RepositoryMissing
    {
        return Err(HandoffError::RepositoryMissing {
            id: item_identifier,
            path: scope.repository_root,
        });
    }
    Ok(Some(GateItem {
        pending_work_identifier: identifier.to_string(),
        scope,
    }))
}

fn documents_location<S>(
    store: &S,
    gate: &GateItem,
    location: HandoffLocation,
) -> Result<Vec<HandoffDocument>, HandoffError>
where
    S: HandoffDocumentStore,
{
    store
        .list_location(&gate.scope, location)
        .map_err(|source| HandoffError::ReadDocuments {
            path: handoff_directory(&gate.scope, location),
            source: Box::new(source),
        })
}

fn find_linked<'documents>(
    documents: &'documents [HandoffDocument],
    pending_work_identifier: &str,
    location: HandoffLocation,
    status_matches: impl Fn(&HandoffDocument) -> bool,
    directory: &Path,
) -> Result<Option<&'documents HandoffDocument>, HandoffError> {
    let mut matches = documents.iter().filter(|document| {
        document.location == location
            && status_matches(document)
            && document.pending_work_identifier_raw.as_deref().is_some_and(
                |linked_pending_work_identifier| {
                    linked_pending_work_identifier.eq_ignore_ascii_case(pending_work_identifier)
                },
            )
    });
    let first = matches.next();
    if matches.next().is_some() {
        return Err(HandoffError::AmbiguousHandoff {
            id: pending_work_identifier.to_string(),
            directory: directory.to_path_buf(),
        });
    }
    Ok(first)
}

pub(crate) fn newest_handoff<S>(
    store: &S,
    repository_root: &str,
) -> Result<(String, String), HandoffError>
where
    S: HandoffDocumentStore,
{
    let scope = scope(repository_root);
    let directory = handoff_directory(&scope, HandoffLocation::Active);
    if store
        .scope_presence(&scope)
        .map_err(|source| HandoffError::ReadDocuments {
            path: directory.clone(),
            source: Box::new(source),
        })?
        != HandoffDocumentScopePresence::Present
    {
        return Err(HandoffError::HandoffDirectoryMissing { path: directory });
    }
    let documents = store
        .list_location(&scope, HandoffLocation::Active)
        .map_err(|source| HandoffError::ReadDocuments {
            path: directory.clone(),
            source: Box::new(source),
        })?;
    let document = documents
        .into_iter()
        .filter(|document| document.location == HandoffLocation::Active)
        .max_by(|left, right| {
            left.modified_timestamp
                .cmp(&right.modified_timestamp)
                .then_with(|| left.identifier.file_name.cmp(&right.identifier.file_name))
        })
        .ok_or_else(|| HandoffError::HandoffMarkdownMissing {
            path: directory.clone(),
        })?;
    let relative = document
        .locator
        .strip_prefix(&scope.repository_root)
        .unwrap_or(&document.locator);
    let relative = path_forward(relative);
    Ok((
        naming::continuation_title(&document.identifier.file_name),
        handoff_continuation_prompt(&relative),
    ))
}

fn handoff_file_name(created: &str, title: &str) -> String {
    naming::file_name(created, title)
}

fn handoff_continuation_prompt(relative_path: &str) -> String {
    format!("Continue the handoff at @{relative_path}.")
}

fn path_forward(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn cancelled_body(body: &str, report: &str) -> String {
    format!("{}\n\n> Cancelled: {report}\n", body.trim_end())
}
