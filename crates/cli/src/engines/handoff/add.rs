//! Translates `handoff add` arguments into the application operation.

use std::{
    fmt::Write as _,
    path::{Path, PathBuf},
};

use clap::Args;
use pwf_application::{
    AppRecordStore, Clock, HandoffDocumentStore, HandoffLedger, IndexEntry, IndexSection,
    PendingWorkItem,
    handoff::{
        add_handoff::{self, AddHandoff, AddHandoffError, HandoffAllocation},
        ports::PendingWorkAllocatorClient,
    },
    pending_work::ProjectRegistry,
};
use pwf_infra::{obsidian::ObsidianStore, pending_work_allocator::ProcessPendingWorkAllocator};

use super::common::{CommonArguments, HandoffError, repository_root};

#[derive(Args, Debug)]
pub struct Arguments {
    /// Handoff title (required).
    #[arg(long)]
    pub(crate) title: Option<String>,
    /// Filename slug (else derived from the title).
    #[arg(long)]
    pub(crate) slug: Option<String>,
    #[command(flatten)]
    pub(crate) common: CommonArguments,
}

pub(super) fn run(
    arguments: &Arguments,
    store: &ObsidianStore,
    projects: &ProjectRegistry,
    clock: &impl Clock,
) -> Result<String, HandoffError> {
    let root = repository_root(&arguments.common)?;
    let allocator = ProcessPendingWorkAllocator::new(
        arguments
            .common
            .pending_work_script
            .as_deref()
            .map(PathBuf::from),
    );
    let allocation = if arguments.common.pending_work_script.is_some() {
        HandoffAllocation::External
    } else {
        HandoffAllocation::InProcess
    };
    invoke_add(
        &root, arguments, allocation, store, projects, &allocator, clock,
    )
}

pub(in crate::engines::handoff) fn invoke_add<S, A, C>(
    root: &Path,
    args: &Arguments,
    allocation: HandoffAllocation,
    store: &S,
    projects: &ProjectRegistry,
    allocator: &A,
    clock: &C,
) -> Result<String, HandoffError>
where
    S: AppRecordStore<PendingWorkItem>
        + AppRecordStore<IndexEntry>
        + AppRecordStore<IndexSection>
        + HandoffDocumentStore
        + AppRecordStore<HandoffLedger>,
    A: PendingWorkAllocatorClient,
    C: Clock,
{
    let title = args.title.as_deref().ok_or(HandoffError::MissingTitle)?;
    let added = add_handoff::execute(
        AddHandoff {
            scope: pwf_application::HandoffScope {
                repository_root: root.to_path_buf(),
            },
            title: title.to_string(),
            slug: args.slug.clone(),
            date: args.common.date.clone(),
            allocation,
        },
        store,
        projects,
        allocator,
        clock,
    )
    .map_err(|source| match source {
        AddHandoffError::ProjectResolution { .. } => HandoffError::UnmanagedRepo {
            root: root.display().to_string(),
        },
        AddHandoffError::DestinationExists { path } => HandoffError::HandoffAlreadyExists { path },
        source => HandoffError::Add { source },
    })?;

    let id = added.pending_work_identifier;
    let mut out = format!("Created handoff {}", added.handoff_path.display());
    let _ = write!(out, "\n  pw: {id}");
    let _ = write!(
        out,
        "\n  Now fill the Goals + Context; close with: pwf done --id {id}"
    );
    Ok(out)
}
