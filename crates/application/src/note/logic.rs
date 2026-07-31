use pwf_models::{
    note::NoteId,
    pending_work::{ProjectId, ProjectName},
};

use crate::pending_work::ProjectRegistry;

pub(super) struct ResolvedProject {
    pub project_name: ProjectName,
    pub project_id: ProjectId,
}

pub(super) fn resolve_project(projects: &ProjectRegistry, raw: &str) -> Option<ResolvedProject> {
    let project_name = projects.resolve(raw).ok()?.clone();
    let project_id = projects.get_project_id_by(&project_name)?;
    Some(ResolvedProject {
        project_name,
        project_id,
    })
}

pub(super) fn resolve_note(raw: &str, project_id: &ProjectId) -> Option<NoteId> {
    let raw_uppercase = raw.trim().to_ascii_uppercase();
    let full_prefix = format!("{project_id}-NOTE-");
    let number = raw_uppercase
        .strip_prefix(&full_prefix)
        .or_else(|| raw_uppercase.strip_prefix("NOTE-"))
        .unwrap_or(&raw_uppercase)
        .parse::<u32>()
        .ok()?;
    NoteId::try_new(format!("{project_id}-NOTE-{number:04}")).ok()
}
