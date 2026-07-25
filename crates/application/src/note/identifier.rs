use pwf_domain::{
    note::NoteId,
    pending_work::{ProjectName, ProjectPrefix},
};

use crate::pending_work::ProjectRegistry;

pub(super) struct ResolvedProject {
    pub project: ProjectName,
    pub prefix: ProjectPrefix,
}

pub(super) fn resolve_project(projects: &ProjectRegistry, raw: &str) -> Option<ResolvedProject> {
    let project = projects.resolve(raw).ok()?.clone();
    let prefix = projects.prefix_for(&project)?;
    Some(ResolvedProject { project, prefix })
}

pub(super) fn resolve_note(raw: &str, prefix: &ProjectPrefix) -> Option<NoteId> {
    let raw_uppercase = raw.trim().to_ascii_uppercase();
    let full_prefix = format!("{prefix}-NOTE-");
    let number = raw_uppercase
        .strip_prefix(&full_prefix)
        .or_else(|| raw_uppercase.strip_prefix("NOTE-"))
        .unwrap_or(&raw_uppercase)
        .parse::<u32>()
        .ok()?;
    NoteId::try_new(format!("{prefix}-NOTE-{number:04}")).ok()
}
