//! Translates `handoff add` arguments into the application operation.

use std::{
    fmt::Write as _,
    path::{Path, PathBuf},
};

use clap::Args;
use pwf_application::{
    AppDbStore, HandoffDocumentStore, HandoffLedger, IndexEntry, IndexSection, PendingWorkItem,
    handoff::{
        add::{AddHandoff, AddHandoffError, HandoffAllocation},
        ports::PendingWorkAllocatorClient,
    },
    pending_work::ProjectRegistry,
};
use pwf_domain::pending_work::ProjectName;
use pwf_infra::{obsidian::ObsidianStore, pending_work_allocator::ProcessPendingWorkAllocator};

use super::common::{CommonArguments, HandoffError, date, repository_root};

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

pub(super) fn run(arguments: &Arguments) -> Result<String, HandoffError> {
    let root = repository_root(&arguments.common)?;
    let title = arguments
        .title
        .as_deref()
        .ok_or(HandoffError::MissingTitle)?;
    let config_path = arguments
        .common
        .config_path
        .clone()
        .or_else(crate::config::default_config_path)
        .unwrap_or_default();
    let configuration = crate::config::load(&config_path, None)
        .map_err(|source| HandoffError::Config { source })?;
    let projects = ProjectRegistry::new(configuration.projects.iter().map(|(name, repository)| {
        (
            ProjectName::try_new(name).expect("configured project is non-empty"),
            Some(repository.clone()),
            configuration
                .prefixes
                .get(name)
                .map(|prefix| prefix.to_ascii_uppercase()),
        )
    }));
    let store = ObsidianStore::new(configuration);
    let allocator = ProcessPendingWorkAllocator::new(
        arguments
            .common
            .pending_work_script
            .as_deref()
            .map(PathBuf::from),
    );
    let allocation = if arguments.common.pending_work_script.is_some() {
        HandoffAllocation::External {
            config_path: PathBuf::from(config_path),
        }
    } else {
        HandoffAllocation::InProcess
    };
    invoke_add(
        &root, arguments, title, allocation, &store, &projects, &allocator,
    )
}

pub(in crate::engines::handoff) fn invoke_add<S, C>(
    root: &Path,
    args: &Arguments,
    title: &str,
    allocation: HandoffAllocation,
    store: &S,
    projects: &ProjectRegistry,
    allocator: &C,
) -> Result<String, HandoffError>
where
    S: AppDbStore<PendingWorkItem>
        + AppDbStore<IndexEntry>
        + AppDbStore<IndexSection>
        + HandoffDocumentStore
        + AppDbStore<HandoffLedger>,
    C: PendingWorkAllocatorClient,
{
    let today = date(args.common.date.as_deref());
    let added = pwf_application::handoff::add::execute(
        AddHandoff {
            scope: pwf_application::HandoffScope {
                repository_root: root.to_path_buf(),
            },
            title: title.to_string(),
            slug: args.slug.clone(),
            created: today,
            allocation,
        },
        store,
        projects,
        allocator,
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
