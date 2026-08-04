use lazy_regex::{Regex, regex};
use pwf_models::{
    project::Project,
    task::{PrerequisiteInput, PrerequisiteInputError, Prerequisites, TaskId, TaskStatus},
};
use pwf_wire::task::PrerequisiteStatus;

use crate::ports::task_record::{Materialization, TaskRecord, TaskStore};

fn prerequisite_value_regex() -> &'static Regex {
    regex!(r"\[\[([A-Z]{3}-\d{4})")
}

fn persisted_status_regex() -> &'static Regex {
    regex!(r"(?m)^status:\s*([^\r\n]+)$")
}

fn persisted_frontmatter_regex() -> &'static Regex {
    regex!(r"(?s)\A(?:\u{feff})?---[ \t]*\r?\n(.*?)\r?\n---[ \t]*(?:\r?\n|\z)")
}

#[derive(Debug, thiserror::Error)]
pub(in crate::task) enum PrerequisiteValidationError {
    #[error("Unknown --prereq id(s): {}.", format_task_ids(ids))]
    UnknownIds { ids: Vec<TaskId> },
}

fn format_task_ids(ids: &[TaskId]) -> String {
    ids.iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

pub(in crate::task) fn project_ids(values: &Prerequisites) -> Vec<pwf_models::project::ProjectId> {
    let mut projects = Vec::new();
    for id in values.iter() {
        let project = id.project_id();
        if !projects.contains(&project) {
            projects.push(project);
        }
    }
    projects
}

pub(in crate::task) fn referenced_project_ids<'a>(
    values: impl IntoIterator<Item = &'a str>,
) -> Vec<pwf_models::project::ProjectId> {
    let mut projects = Vec::new();
    for id in values
        .into_iter()
        .flat_map(|value| prerequisite_value_regex().captures_iter(value))
        .filter_map(|captures| TaskId::try_new(&captures[1]).ok())
    {
        let project = id.project_id();
        if !projects.contains(&project) {
            projects.push(project);
        }
    }
    projects
}

pub(in crate::task) fn extract(raw: &str) -> Option<Prerequisites> {
    let identifiers = prerequisite_value_regex()
        .captures_iter(raw)
        .filter_map(|captures| TaskId::try_new(&captures[1]).ok())
        .collect::<Vec<_>>();
    Prerequisites::try_new(identifiers).ok()
}

pub(in crate::task) fn parse_frontmatter(raw: &str) -> Result<Vec<TaskId>, PrerequisiteInputError> {
    let raw = raw.trim();
    let unquoted = raw
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            raw.strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
        .unwrap_or(raw);
    let input = unquoted.parse::<PrerequisiteInput>()?;
    Ok(input.iter().cloned().collect())
}

pub(in crate::task) fn validate_and_merge(
    existing: Option<&str>,
    prerequisites: &Prerequisites,
    store: &impl TaskStore,
    projects: &[Project],
) -> Result<Prerequisites, PrerequisiteValidationError> {
    let mut unknown = Vec::new();
    for identifier in prerequisites.iter() {
        let Some(project) = projects
            .iter()
            .find(|project| project.id == identifier.project_id())
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
        return Err(PrerequisiteValidationError::UnknownIds { ids: unknown });
    }

    Ok(extract(existing.unwrap_or_default()).map_or_else(
        || prerequisites.clone(),
        |existing| existing.merge(prerequisites),
    ))
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
    prerequisites: &Prerequisites,
    store: &impl TaskStore,
    projects: &[Project],
) -> Vec<PrerequisiteStatus> {
    prerequisites
        .iter()
        .map(|id| {
            let status = status(store, projects, id);
            PrerequisiteStatus {
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
    let project = projects.iter().find(|project| project.id == id.project_id())?;
    // FIXME: Distinguish an absent prerequisite from a read or parse failure; both currently render as "missing" and can hide vault corruption.
    store
        .get(project, id)
        .ok()
        .flatten()
        .filter(|task| !matches!(&task.materialization, Materialization::MissingNote { .. }))
        .map(|task| task.status)
}

#[cfg(test)]
mod tests {
    use pwf_models::task::{PrerequisiteInput, Prerequisites};

    use super::{PrerequisiteValidationError, referenced_project_ids, validate_and_merge};
    use crate::{
        ports::task_record::TaskRecord,
        testing::{project, staged_missing_task, staged_task},
    };

    fn inputs(values: &[&str]) -> Prerequisites {
        let inputs = values
            .iter()
            .map(|value| value.parse::<PrerequisiteInput>().unwrap())
            .collect::<Vec<_>>();
        Prerequisites::from_inputs(&inputs).unwrap()
    }

    #[test]
    fn persisted_values_expose_referenced_projects() {
        assert_eq!(
            referenced_project_ids(["[[CFG-0057]], [[PWF-0001]]", "[[CFG-0014]], ignored",])
                .iter()
                .map(AsRef::as_ref)
                .collect::<Vec<_>>(),
            ["CFG", "PWF"]
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
    fn validator_preserves_prerequisite_diagnostics() {
        let (store, _registry) = staged_task();
        let projects = [project("PWF", "pwf")];

        let unknown =
            validate_and_merge(None, &inputs(&["PWF-9999"]), &store, &projects).unwrap_err();
        assert!(matches!(
            unknown,
            PrerequisiteValidationError::UnknownIds { ref ids }
                if ids.iter().map(AsRef::as_ref).collect::<Vec<_>>() == ["PWF-9999"]
        ));
        assert_eq!(unknown.to_string(), "Unknown --prereq id(s): PWF-9999.");
    }

    #[test]
    fn validator_rejects_missing_note_materializations_as_unknown() {
        let (store, _registry) = staged_missing_task();
        let projects = [project("PWF", "pwf")];

        let error =
            validate_and_merge(None, &inputs(&["PWF-0002"]), &store, &projects).unwrap_err();

        assert!(matches!(
            error,
            PrerequisiteValidationError::UnknownIds { ref ids }
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
                PrerequisiteValidationError::UnknownIds { ref ids }
                    if ids.iter().map(AsRef::as_ref).collect::<Vec<_>>() == ["PWF-0001"]
            ));
        }
    }
}
