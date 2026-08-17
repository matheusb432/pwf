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
    #[error("cannot read --blocked-by task {id}: {source}")]
    ReadStore {
        id: TaskId,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

#[derive(Debug, thiserror::Error)]
pub(in crate::task) enum BlockedByStatusError {
    #[error("cannot read blocked-by task {id}: {source}")]
    ReadStore {
        id: TaskId,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
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
        let record = store.get(project, identifier).map_err(|source| {
            BlockedByValidationError::ReadStore {
                id: identifier.clone(),
                source: Box::new(source),
            }
        })?;
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
) -> Result<Vec<BlockedByStatus>, BlockedByStatusError> {
    blocked_by
        .iter()
        .map(|id| {
            let status = status(store, projects, id)?;
            Ok(BlockedByStatus {
                id: id.clone(),
                status,
            })
        })
        .collect()
}

fn status(
    store: &impl TaskStore,
    projects: &[Project],
    id: &TaskId,
) -> Result<Option<TaskStatus>, BlockedByStatusError> {
    let Some(project) = projects
        .iter()
        .find(|project| &project.id == id.project_id())
    else {
        return Ok(None);
    };
    let task = store
        .get(project, id)
        .map_err(|source| BlockedByStatusError::ReadStore {
            id: id.clone(),
            source: Box::new(source),
        })?;
    let Some(task) = task else {
        return Ok(None);
    };
    if matches!(task.materialization, Materialization::MissingNote { .. }) {
        return Ok(None);
    }
    Ok(Some(task.status))
}

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use pwf_models::{
        project::Project,
        task::{BlockedBy, BlockedByInput, TaskId},
    };

    use super::{
        BlockedByStatusError, BlockedByValidationError, referenced_project_ids, statuses,
        validate_and_merge,
    };
    use crate::{
        ports::task_record::{NewTask, TaskPatch, TaskRecord, TaskStore},
        testing::{project, staged_missing_task, staged_task},
    };

    #[derive(Debug, Clone, Copy, thiserror::Error)]
    #[error("vault read failed")]
    struct FailingStoreError;

    #[derive(Clone, Copy)]
    struct FailingStore;

    impl TaskStore for FailingStore {
        type Error = FailingStoreError;

        fn get(&self, _project: &Project, _id: &TaskId) -> Result<Option<TaskRecord>, Self::Error> {
            Err(FailingStoreError)
        }

        fn list(&self, _project: &Project) -> Result<Vec<TaskRecord>, Self::Error> {
            Err(FailingStoreError)
        }

        fn insert(&self, _project: &Project, _new: NewTask) -> Result<TaskRecord, Self::Error> {
            Err(FailingStoreError)
        }

        fn update(
            &self,
            _project: &Project,
            _id: &TaskId,
            _patch: TaskPatch,
        ) -> Result<(), Self::Error> {
            Err(FailingStoreError)
        }

        fn delete(&self, _project: &Project, _id: &TaskId) -> Result<(), Self::Error> {
            Err(FailingStoreError)
        }
    }

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
                "[[PW-0002]], [[AUX-0057]], [[PWF-0001]]",
                "[[AUX-0014]], [[TOOL-0042]], ignored",
            ])
            .iter()
            .map(AsRef::as_ref)
            .collect::<Vec<_>>(),
            ["PW", "AUX", "PWF", "TOOL"]
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
    fn validator_preserves_store_read_failures() {
        let projects = [project("PWF", "pwf")];

        let error =
            validate_and_merge(None, &inputs(&["PWF-0001"]), &FailingStore, &projects).unwrap_err();

        assert!(matches!(
            error,
            BlockedByValidationError::ReadStore { ref id, .. } if id.as_ref() == "PWF-0001"
        ));
        assert_eq!(error.source().unwrap().to_string(), "vault read failed");
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

    #[test]
    fn statuses_preserve_store_read_failures() {
        let projects = [project("PWF", "pwf")];

        let error = statuses(&inputs(&["PWF-0001"]), &FailingStore, &projects).unwrap_err();

        assert!(matches!(
            error,
            BlockedByStatusError::ReadStore { ref id, .. } if id.as_ref() == "PWF-0001"
        ));
        assert_eq!(error.source().unwrap().to_string(), "vault read failed");
    }
}
