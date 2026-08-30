mod active_task;
pub mod add_task;
mod blocked_by;
pub mod cancel_task;
pub mod complete_task;
pub mod edit_task;
pub mod get_task;
pub mod list_tasks;
pub mod migrate_task_metadata;
mod mutation_request;
mod note_body;
pub mod remove_task;
pub mod reopen_task;
pub mod resolve_task_project;
pub mod session;
mod tags;
mod task_closure;
mod task_creation;
mod task_view;

pub use mutation_request::MutationRequestError;
pub use task_closure::CloseTaskError;

fn infer_task_title(
    prompt: &pwf_models::task::TaskPrompt,
) -> Result<pwf_models::task::TaskTitle, pwf_models::task::TaskTitleError> {
    pwf_models::task::TaskTitle::try_new(prompt_lanes::parse(prompt.as_ref()).title)
}

fn task_body_region(body: &str) -> &str {
    body.strip_prefix('\n').unwrap_or(body)
}

fn task_revision(record: &crate::ports::task_record::TaskRecord) -> pwf_wire::task::TaskRevision {
    use crate::ports::task_record::{Materialization, StoredBlockedBy};

    let mut hasher = blake3::Hasher::new();
    revision_field(&mut hasher, "id", record.id.as_ref());
    revision_field(&mut hasher, "title", &record.title);
    revision_field(
        &mut hasher,
        "status",
        match record.status {
            pwf_models::task::TaskStatus::Active => "active",
            pwf_models::task::TaskStatus::Done => "done",
            pwf_models::task::TaskStatus::Cancelled => "cancelled",
        },
    );
    revision_optional_field(
        &mut hasher,
        "created_at",
        record.created_at.map(|value| value.to_string()).as_deref(),
    );
    revision_optional_field(
        &mut hasher,
        "completed_at",
        record
            .completed_at
            .map(|value| value.to_string())
            .as_deref(),
    );
    revision_optional_field(&mut hasher, "commits", record.commits.as_deref());
    revision_optional_field(&mut hasher, "tags", record.tags.as_ref().map(AsRef::as_ref));
    revision_optional_field(&mut hasher, "effort", record.effort.as_deref());
    revision_optional_field(&mut hasher, "priority", record.priority.as_deref());
    match &record.blocked_by {
        StoredBlockedBy::Absent => revision_field(&mut hasher, "blocked_by", "absent"),
        StoredBlockedBy::Valid(values) => {
            revision_field(&mut hasher, "blocked_by", "valid");
            for value in values.iter() {
                revision_field(&mut hasher, "blocker", value.as_ref());
            }
        }
        StoredBlockedBy::Malformed { raw, reason } => {
            revision_field(&mut hasher, "blocked_by", "malformed");
            revision_field(&mut hasher, "blocked_by_raw", raw);
            revision_field(&mut hasher, "blocked_by_reason", reason);
        }
    }
    revision_optional_field(
        &mut hasher,
        "section",
        record.section.as_ref().map(AsRef::as_ref),
    );
    revision_field(&mut hasher, "body", &record.body);
    revision_field(&mut hasher, "locator", &record.locator.to_string());
    match &record.materialization {
        Materialization::NoteFile => revision_field(&mut hasher, "materialization", "note"),
        Materialization::MissingNote { expected } => {
            revision_field(&mut hasher, "materialization", "missing");
            revision_field(&mut hasher, "expected", &expected.to_string());
        }
    }
    pwf_wire::task::TaskRevision::from_digest(*hasher.finalize().as_bytes())
}

fn revision_optional_field(hasher: &mut blake3::Hasher, name: &str, value: Option<&str>) {
    match value {
        Some(value) => revision_field(hasher, name, value),
        None => revision_field(hasher, name, "<absent>"),
    }
}

fn revision_field(hasher: &mut blake3::Hasher, name: &str, value: &str) {
    hasher.update(&revision_length(name.len()));
    hasher.update(name.as_bytes());
    hasher.update(&revision_length(value.len()));
    hasher.update(value.as_bytes());
}

fn revision_length(length: usize) -> [u8; 8] {
    u64::try_from(length).unwrap_or(u64::MAX).to_le_bytes()
}

#[derive(Debug, thiserror::Error)]
#[error(
    "task changed since it was read (expected revision {expected}, current revision {current})"
)]
pub struct TaskRevisionConflict {
    expected: pwf_wire::task::TaskRevision,
    current: pwf_wire::task::TaskRevision,
}

fn ensure_task_revision(
    expected: Option<&pwf_wire::task::TaskRevision>,
    record: &crate::ports::task_record::TaskRecord,
) -> Result<(), TaskRevisionConflict> {
    let Some(expected) = expected else {
        return Ok(());
    };
    let current = task_revision(record);
    if expected == &current {
        return Ok(());
    }
    Err(TaskRevisionConflict {
        expected: expected.clone(),
        current,
    })
}

fn section_alias(label: &str) -> Option<&'static str> {
    match label.trim().to_ascii_lowercase().as_str() {
        "future" | "futuro" => Some("Future"),
        "human" => Some("Human"),
        "low-prio" | "low-priority" => Some("Low-prio"),
        _ => None,
    }
}

fn normalize_section_label(label: &pwf_models::task::TaskSection) -> pwf_models::task::TaskSection {
    match section_alias(label.as_ref()) {
        Some("Future") => pwf_models::task::TaskSection::future(),
        Some("Human") => pwf_models::task::TaskSection::human(),
        Some("Low-prio") => pwf_models::task::TaskSection::low_priority(),
        Some(_) | None => label.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::{normalize_section_label, section_alias};

    #[test]
    fn section_aliases_map_only_known_labels() {
        assert_eq!(section_alias(" Futuro "), Some("Future"));
        assert_eq!(section_alias("future"), Some("Future"));
        assert_eq!(section_alias("HUMAN"), Some("Human"));
        assert_eq!(section_alias("low-priority"), Some("Low-prio"));
        assert_eq!(section_alias("Someday"), None);
        let someday = " SomeDay ".parse().unwrap();
        assert_eq!(normalize_section_label(&someday).as_ref(), "SomeDay");
    }
}
