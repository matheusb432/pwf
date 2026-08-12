use lazy_regex::{Regex, regex};
use pwf_models::{
    project::Project,
    task::{BlockedBy, BlockedByInput, BlockedByInputError, TaskId, TaskStatus},
};
use pwf_wire::task::BlockedByStatus;

use crate::ports::task_record::{Materialization, TaskRecord, TaskStore};

fn blocked_by_value_regex() -> &'static Regex {
    regex!(r"\[\[([A-Z]{2,4}-\d{4})")
}

fn persisted_status_regex() -> &'static Regex {
    regex!(r"(?m)^status:\s*([^\r\n]+)$")
}

fn persisted_frontmatter_regex() -> &'static Regex {
    regex!(r"(?s)\A(?:\u{feff})?---[ \t]*\r?\n(.*?)\r?\n---[ \t]*(?:\r?\n|\z)")
}

#[derive(Debug, thiserror::Error)]
pub(in crate::task) enum BlockedByValidationError {
    #[error("Unknown --blocked-by id(s): {}.", format_task_ids(ids))]
    UnknownIds { ids: Vec<TaskId> },
}

pub(in crate::task) fn format_task_ids(ids: &[TaskId]) -> String {
    ids.iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

pub(in crate::task) fn project_ids(values: &BlockedBy) -> Vec<pwf_models::project::ProjectId> {
    unique_project_ids(values.iter().cloned())
}

pub(in crate::task) fn referenced_project_ids<'a>(
    values: impl IntoIterator<Item = &'a str>,
) -> Vec<pwf_models::project::ProjectId> {
    unique_project_ids(
        values
            .into_iter()
            .flat_map(|value| blocked_by_value_regex().captures_iter(value))
            .filter_map(|captures| TaskId::try_new(&captures[1]).ok()),
    )
}

fn unique_project_ids(
    identifiers: impl IntoIterator<Item = TaskId>,
) -> Vec<pwf_models::project::ProjectId> {
    let mut projects = Vec::new();
    for id in identifiers {
        let project = id.project_id().clone();
        if !projects.contains(&project) {
            projects.push(project);
        }
    }
    projects
}

pub(in crate::task) fn extract(raw: &str) -> Option<BlockedBy> {
    let identifiers = blocked_by_value_regex()
        .captures_iter(raw)
        .filter_map(|captures| TaskId::try_new(&captures[1]).ok())
        .collect::<Vec<_>>();
    BlockedBy::try_new(identifiers).ok()
}

pub(in crate::task) fn parse_frontmatter(raw: &str) -> Result<BlockedBy, BlockedByInputError> {
    let raw = raw.trim();
    let unquoted = raw
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            raw.strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
        .unwrap_or(raw);
    let input = unquoted.parse::<BlockedByInput>()?;
    let blocked_by = BlockedBy::from_inputs(&[input]).ok_or(BlockedByInputError::MissingId)?;
    Ok(blocked_by)
}

pub(in crate::task) fn validate_and_merge(
    existing: Option<&str>,
    blocked_by: &BlockedBy,
    store: &impl TaskStore,
    projects: &[Project],
) -> Result<BlockedBy, BlockedByValidationError> {
    let mut unknown = Vec::new();
    for identifier in blocked_by.iter() {
        let Some(project) = projects
            .iter()
            .find(|project| &project.id == identifier.project_id())
        else {
            unknown.push(identifier.clone());
            continue;
        };
        let Ok(record) = store.get(project, identifier) else {
            unknown.push(identifier.clone());
            continue;
        };
        if record.is_none_or(|record| !has_valid_persisted_status(&record)) {
            unknown.push(identifier.clone());
        }
    }
    if !unknown.is_empty() {
        return Err(BlockedByValidationError::UnknownIds { ids: unknown });
    }

    Ok(extract(existing.unwrap_or_default())
        .map_or_else(|| blocked_by.clone(), |existing| existing.merge(blocked_by)))
}

fn has_valid_persisted_status(record: &TaskRecord) -> bool {
    matches!(record.materialization, Materialization::NoteFile)
        && persisted_frontmatter_regex()
            .captures(&record.source)
            .and_then(|captures| captures.get(1))
            .and_then(|frontmatter| persisted_status_regex().captures(frontmatter.as_str()))
            .and_then(|captures| captures.get(1))
            .is_some_and(|value| value.as_str().trim().parse::<TaskStatus>().is_ok())
}

pub(in crate::task) fn statuses(
    blocked_by: &BlockedBy,
    store: &impl TaskStore,
    projects: &[Project],
) -> Vec<BlockedByStatus> {
    blocked_by
        .iter()
        .map(|id| {
            let status = status(store, projects, id);
            BlockedByStatus {
                id: id.clone(),
                status,
            }
        })
        .collect()
}

#[rustfmt::skip]
fn status(
    store: &impl TaskStore,
    projects: &[Project],
    id: &TaskId,
) -> Option<TaskStatus> {
    let project = projects.iter().find(|project| &project.id == id.project_id())?;
    // FIXME: Distinguish an absent blocked-by task from a read or parse failure; both currently render as "missing" and can hide vault corruption.
    store
        .get(project, id)
        .ok()
        .flatten()
        .filter(|task| !matches!(&task.materialization, Materialization::MissingNote { .. }))
        .map(|task| task.status)
}

#[cfg(test)]
mod tests {
    use pwf_models::task::{BlockedBy, BlockedByInput};

    use super::{BlockedByValidationError, referenced_project_ids, validate_and_merge};
    use crate::{
        ports::task_record::TaskRecord,
        testing::{project, staged_missing_task, staged_task},
    };

    fn inputs(values: &[&str]) -> BlockedBy {
        let inputs = values
            .iter()
            .map(|value| value.parse::<BlockedByInput>().unwrap())
            .collect::<Vec<_>>();
        BlockedBy::from_inputs(&inputs).unwrap()
    }

    #[test]
    fn persisted_values_expose_referenced_projects() {
        assert_eq!(
            referenced_project_ids([
                "[[PW-0002]], [[CFG-0057]], [[PWF-0001]]",
                "[[CFG-0014]], [[TOOL-0042]], ignored",
            ])
            .iter()
            .map(AsRef::as_ref)
            .collect::<Vec<_>>(),
            ["PW", "CFG", "PWF", "TOOL"]
        );
    }

    #[test]
    fn validator_parses_checks_existence_and_merges_first_seen_ids() {
        let (store, _registry) = staged_task();
        let projects = [project("PWF", "pwf")];
        let merged = validate_and_merge(
            Some("[[PWF-0001]], [[PWF-0001]]"),
            &inputs(&["pwf1, PWF-0001"]),
            &store,
            &projects,
        )
        .unwrap();

        assert_eq!(merged.to_string(), "[[PWF-0001]]");
    }

    #[test]
    fn validator_preserves_blocked_by_diagnostics() {
        let (store, _registry) = staged_task();
        let projects = [project("PWF", "pwf")];

        let unknown =
            validate_and_merge(None, &inputs(&["PWF-9999"]), &store, &projects).unwrap_err();
        assert!(matches!(
            unknown,
            BlockedByValidationError::UnknownIds { ref ids }
                if ids.iter().map(AsRef::as_ref).collect::<Vec<_>>() == ["PWF-9999"]
        ));
        assert_eq!(unknown.to_string(), "Unknown --blocked-by id(s): PWF-9999.");
    }

    #[test]
    fn validator_rejects_missing_note_materializations_as_unknown() {
        let (store, _registry) = staged_missing_task();
        let projects = [project("PWF", "pwf")];

        let error =
            validate_and_merge(None, &inputs(&["PWF-0002"]), &store, &projects).unwrap_err();

        assert!(matches!(
            error,
            BlockedByValidationError::UnknownIds { ref ids }
                if ids.iter().map(AsRef::as_ref).collect::<Vec<_>>() == ["PWF-0002"]
        ));
    }

    #[test]
    fn validator_rejects_missing_or_invalid_persisted_status_as_unknown() {
        let (staged_store, _registry) = staged_task();
        let projects = [project("PWF", "pwf")];
        let base = staged_store.tasks("pwf")[0].clone();
        for source in [
            "---\nid: PWF-0001\ntitle: task\n---\n\nbody\n",
            "---\nid: PWF-0001\nstatus: paused\ntitle: task\n---\n\nbody\n",
            "---\nid: PWF-0001\ntitle: task\n---\n\nstatus: active\n",
        ] {
            let record = TaskRecord {
                source: source.to_string(),
                ..base.clone()
            };
            let store = crate::testing::InMemoryStore::default().with_project("pwf", vec![record]);

            let error =
                validate_and_merge(None, &inputs(&["PWF-0001"]), &store, &projects).unwrap_err();

            assert!(matches!(
                error,
                BlockedByValidationError::UnknownIds { ref ids }
                    if ids.iter().map(AsRef::as_ref).collect::<Vec<_>>() == ["PWF-0001"]
            ));
        }
    }
}
