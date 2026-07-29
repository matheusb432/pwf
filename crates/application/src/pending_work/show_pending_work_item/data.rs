use pwf_models::pending_work::{EffortTier, ProjectName};

use super::{PendingWorkItemData, ShowPendingWorkError};
use crate::{
    PendingWorkRecord, RecordId,
    pending_work::{prerequisite, tag_policy},
};

pub(super) fn from_record(
    project: ProjectName,
    record: PendingWorkRecord,
) -> Result<PendingWorkItemData, ShowPendingWorkError> {
    let tags = record
        .tags
        .as_deref()
        .map(tag_policy::parse_frontmatter)
        .transpose()
        .map_err(|error| invalid_item_data("tags", error))?;
    let effort = record.effort.as_deref().map(parse_effort).transpose()?;
    let prerequisites = record
        .prereq
        .as_deref()
        .map(prerequisite::parse_frontmatter)
        .transpose()
        .map_err(|error| invalid_item_data("prerequisites", error))?;
    let id = match record.id {
        RecordId::Item(id) => id.to_string(),
        RecordId::Inline(ordinal) => format!("{project}:{ordinal}"),
    };

    Ok(PendingWorkItemData {
        id,
        project,
        title: record.title,
        status: record.status,
        created: record.created,
        completed: record.completed,
        commits: record
            .commits
            .map(|value| unquote_scalar(&value).to_string()),
        tags,
        effort,
        prerequisites,
        section: record.section,
        prompt: record.body.trim().to_string(),
    })
}

fn unquote_scalar(raw: &str) -> &str {
    raw.strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            raw.strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
        .unwrap_or(raw)
}

fn parse_effort(raw: &str) -> Result<EffortTier, ShowPendingWorkError> {
    raw.trim()
        .parse()
        .map_err(|error| invalid_item_data("effort", error))
}

fn invalid_item_data(field: &'static str, error: impl std::fmt::Display) -> ShowPendingWorkError {
    ShowPendingWorkError::InvalidItemData {
        field,
        reason: error.to_string(),
    }
}
