use std::sync::LazyLock;

use pwf_domain::pending_work::{WorkItemId, WorkItemStatus};
use regex::Regex;

use super::{get_pending_work::PrerequisiteStatus, identifier, project_registry::ProjectRegistry};
use crate::{AppRecordStore, Materialization, PendingWorkItem};

const PREREQUISITE_VALUE_PATTERN: &str = r"\[\[([A-Z]{2,4}-\d{4})";
static PREREQUISITE_VALUE_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(PREREQUISITE_VALUE_PATTERN).expect("valid prerequisite value regex")
});
static PERSISTED_STATUS_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^status:\s*([^\r\n]+)$").expect("valid persisted status regex")
});
static PERSISTED_FRONTMATTER_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)\A(?:\u{feff})?---[ \t]*\r?\n(.*?)\r?\n---[ \t]*(?:\r?\n|\z)")
        .expect("valid persisted frontmatter regex")
});

#[derive(Debug, thiserror::Error)]
pub(super) enum PrerequisiteValidationError {
    #[error("Invalid --prereq id: {raw}.")]
    InvalidId { raw: String },
    #[error("--prereq requires an id.")]
    MissingId,
    #[error("Unknown --prereq id(s): {}.", ids.join(", "))]
    UnknownIds { ids: Vec<String> },
}

fn parse_values(values: &[String]) -> Result<Vec<WorkItemId>, PrerequisiteValidationError> {
    let mut identifiers = Vec::new();
    for value in values {
        for raw in value.split(',') {
            let raw = raw.trim();
            if raw.is_empty() {
                continue;
            }
            let candidate = raw
                .strip_prefix("[[")
                .and_then(|trimmed| trimmed.strip_suffix("]]"))
                .unwrap_or(raw);
            let identifier = identifier::parse(candidate).ok_or_else(|| {
                PrerequisiteValidationError::InvalidId {
                    raw: raw.to_string(),
                }
            })?;
            if !identifiers.contains(&identifier) {
                identifiers.push(identifier);
            }
        }
    }
    if identifiers.is_empty() {
        return Err(PrerequisiteValidationError::MissingId);
    }
    Ok(identifiers)
}

pub(super) fn parse_frontmatter(raw: &str) -> Result<Vec<WorkItemId>, PrerequisiteValidationError> {
    let raw = raw.trim();
    let unquoted = raw
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            raw.strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
        .unwrap_or(raw);
    parse_values(&[unquoted.to_string()])
}

fn frontmatter_value(identifiers: &[WorkItemId]) -> String {
    identifiers
        .iter()
        .map(|identifier| format!("[[{identifier}]]"))
        .collect::<Vec<_>>()
        .join(", ")
}

pub(super) fn validate_and_merge<S>(
    existing: Option<&str>,
    values: &[String],
    store: &S,
    projects: &ProjectRegistry,
) -> Result<String, PrerequisiteValidationError>
where
    S: AppRecordStore<PendingWorkItem>,
{
    let prerequisites = parse_values(values)?;
    let mut unknown = Vec::new();
    for identifier in &prerequisites {
        let Some(project) = projects.project_for_id(identifier) else {
            unknown.push(identifier.as_ref().to_string());
            continue;
        };
        let Ok(record) = store.get(project, identifier) else {
            unknown.push(identifier.as_ref().to_string());
            continue;
        };
        if record.is_none_or(|record| !has_valid_persisted_status(&record)) {
            unknown.push(identifier.as_ref().to_string());
        }
    }
    if !unknown.is_empty() {
        return Err(PrerequisiteValidationError::UnknownIds { ids: unknown });
    }

    let mut identifiers = Vec::new();
    for identifier in existing
        .into_iter()
        .flat_map(|value| PREREQUISITE_VALUE_REGEX.captures_iter(value))
        .map(|captures| {
            WorkItemId::try_new(&captures[1])
                .expect("prerequisite regex captures a canonical work-item id")
        })
        .chain(prerequisites)
    {
        if !identifiers.contains(&identifier) {
            identifiers.push(identifier);
        }
    }
    Ok(frontmatter_value(&identifiers))
}

fn has_valid_persisted_status(record: &PendingWorkItem) -> bool {
    matches!(record.materialization, Materialization::NoteFile)
        && PERSISTED_FRONTMATTER_REGEX
            .captures(&record.source)
            .and_then(|captures| captures.get(1))
            .and_then(|frontmatter| PERSISTED_STATUS_REGEX.captures(frontmatter.as_str()))
            .and_then(|captures| captures.get(1))
            .is_some_and(|value| value.as_str().trim().parse::<WorkItemStatus>().is_ok())
}

pub(super) fn statuses(
    value: &str,
    store: &impl AppRecordStore<PendingWorkItem>,
    projects: &ProjectRegistry,
) -> Vec<PrerequisiteStatus> {
    PREREQUISITE_VALUE_REGEX
        .captures_iter(value)
        .map(|captures| {
            let id = WorkItemId::try_new(&captures[1])
                .expect("prerequisite regex captures a canonical work-item id");
            let status = status(store, projects, &id);
            PrerequisiteStatus { id, status }
        })
        .collect()
}

#[rustfmt::skip]
fn status(
    store: &impl AppRecordStore<PendingWorkItem>,
    projects: &ProjectRegistry,
    id: &WorkItemId,
) -> Option<WorkItemStatus> {
    let project = projects.project_for_id(id)?;
    // FIXME: Distinguish an absent prerequisite from a read or parse failure; both currently render as "missing" and can hide vault corruption.
    store
        .get(project, id)
        .ok()
        .flatten()
        .filter(|item| !matches!(&item.materialization, Materialization::MissingNote { .. }))
        .map(|item| item.status)
}

#[cfg(test)]
mod tests {
    use pwf_domain::pending_work::{ProjectName, WorkItemId};

    use super::{PrerequisiteValidationError, frontmatter_value, parse_values, validate_and_merge};
    use crate::{
        AppRecordStore, ItemPatch, NewItem, PendingWorkItem,
        pending_work::resolve::testing::{staged, staged_ghost},
    };

    #[derive(Clone)]
    struct ReadFailureStore;

    #[derive(Debug, thiserror::Error)]
    #[error("read failed")]
    struct ReadFailure;

    impl AppRecordStore<PendingWorkItem> for ReadFailureStore {
        type Error = ReadFailure;

        fn get(
            &self,
            _scope: &ProjectName,
            _id: &WorkItemId,
        ) -> Result<Option<PendingWorkItem>, Self::Error> {
            Err(ReadFailure)
        }

        fn list(&self, _scope: &ProjectName) -> Result<Vec<PendingWorkItem>, Self::Error> {
            unreachable!("validator reads one prerequisite")
        }

        fn insert(
            &self,
            _scope: &ProjectName,
            _new: NewItem,
        ) -> Result<PendingWorkItem, Self::Error> {
            unreachable!("validator is read-only")
        }

        fn update(
            &self,
            _scope: &ProjectName,
            _id: &WorkItemId,
            _patch: ItemPatch,
        ) -> Result<(), Self::Error> {
            unreachable!("validator is read-only")
        }

        fn delete(&self, _scope: &ProjectName, _id: &WorkItemId) -> Result<(), Self::Error> {
            unreachable!("validator is read-only")
        }
    }

    #[test]
    fn loose_values_normalize_deduplicate_and_render() {
        let prerequisites =
            parse_values(&["cfg57, [[CFG-0014]]".to_string(), "CFG-14".to_string()]).unwrap();

        assert_eq!(
            prerequisites.iter().map(AsRef::as_ref).collect::<Vec<_>>(),
            ["CFG-0057", "CFG-0014"]
        );
        assert_eq!(
            frontmatter_value(&prerequisites),
            "[[CFG-0057]], [[CFG-0014]]"
        );
    }

    #[test]
    fn loose_values_reject_empty_input() {
        assert!(parse_values(&[]).is_err());
        assert!(parse_values(&[" , ".to_string()]).is_err());
    }

    #[test]
    fn validator_parses_checks_existence_and_merges_first_seen_ids() {
        let (store, projects) = staged();
        let merged = validate_and_merge(
            Some("[[PWF-0001]], [[PWF-0001]]"),
            &["pwf1, PWF-0001".to_string()],
            &store,
            &projects,
        )
        .unwrap();

        assert_eq!(merged, "[[PWF-0001]]");
    }

    #[test]
    fn validator_preserves_prerequisite_diagnostics() {
        let (store, projects) = staged();

        let invalid =
            validate_and_merge(None, &["PWF-99999".to_string()], &store, &projects).unwrap_err();
        assert!(matches!(
            invalid,
            PrerequisiteValidationError::InvalidId { ref raw } if raw == "PWF-99999"
        ));
        assert_eq!(invalid.to_string(), "Invalid --prereq id: PWF-99999.");

        let missing =
            validate_and_merge(None, &[", ,".to_string()], &store, &projects).unwrap_err();
        assert!(matches!(missing, PrerequisiteValidationError::MissingId));
        assert_eq!(missing.to_string(), "--prereq requires an id.");

        let unknown =
            validate_and_merge(None, &["PWF-9999".to_string()], &store, &projects).unwrap_err();
        assert!(matches!(
            unknown,
            PrerequisiteValidationError::UnknownIds { ref ids }
                if ids == &vec!["PWF-9999".to_string()]
        ));
        assert_eq!(unknown.to_string(), "Unknown --prereq id(s): PWF-9999.");
    }

    #[test]
    fn validator_rejects_missing_note_materializations_as_unknown() {
        let (store, projects) = staged_ghost();

        let error =
            validate_and_merge(None, &["PWF-0002".to_string()], &store, &projects).unwrap_err();

        assert!(matches!(
            error,
            PrerequisiteValidationError::UnknownIds { ref ids }
                if ids == &vec!["PWF-0002".to_string()]
        ));
    }

    #[test]
    fn validator_classifies_store_read_failures_as_unknown_prerequisites() {
        let (_store, projects) = staged();

        let error = validate_and_merge(
            None,
            &["PWF-0001".to_string()],
            &ReadFailureStore,
            &projects,
        )
        .unwrap_err();

        assert!(matches!(
            error,
            PrerequisiteValidationError::UnknownIds { ref ids }
                if ids == &vec!["PWF-0001".to_string()]
        ));
        assert_eq!(error.to_string(), "Unknown --prereq id(s): PWF-0001.");
    }

    #[test]
    fn validator_rejects_missing_or_invalid_persisted_status_as_unknown() {
        let (staged_store, projects) = staged();
        let base = staged_store.items("pwf")[0].clone();
        for source in [
            "---\nid: PWF-0001\ntitle: task\n---\n\nbody\n",
            "---\nid: PWF-0001\nstatus: paused\ntitle: task\n---\n\nbody\n",
            "---\nid: PWF-0001\ntitle: task\n---\n\nstatus: active\n",
        ] {
            let record = PendingWorkItem {
                source: source.to_string(),
                ..base.clone()
            };
            let store = crate::testing::InMemoryStore::default().with_project("pwf", vec![record]);

            let error =
                validate_and_merge(None, &["PWF-0001".to_string()], &store, &projects).unwrap_err();

            assert!(matches!(
                error,
                PrerequisiteValidationError::UnknownIds { ref ids }
                    if ids == &vec!["PWF-0001".to_string()]
            ));
        }
    }
}
