use pwf_application::{
    AppDbStore, PendingWorkItem,
    pending_work::find::{FindPendingWork, FindPendingWorkError},
};
use pwf_domain::pending_work::ProjectRegistry;
use pwf_infra::obsidian::ObsidianStoreError;

use super::{errors, model::Item};
use crate::config::Config;

pub(super) fn load_config(
    args: &crate::cli::EngineArgs,
) -> Result<Config, errors::PendingWorkError> {
    let cfg_path = resolve_config_path_with_default(args, crate::config::default_config_path)?;
    Ok(crate::config::load(&cfg_path, args.notes_dir.as_deref())?)
}

fn resolve_config_path_with_default(
    args: &crate::cli::EngineArgs,
    default_config_path: impl FnOnce() -> Option<String>,
) -> Result<String, errors::PendingWorkError> {
    args.config_path
        .clone()
        .or_else(default_config_path)
        .ok_or(errors::PendingWorkError::MissingConfigPath)
}

/// Resolves a project's full name or id code, both case-insensitive.
/// Name-prefix abbreviations are rejected as unknown.
///
/// # Errors
///
/// Returns an error when the identifier is unknown or matches several projects.
pub fn resolve_managed_project_name(cfg: &Config, name: &str) -> Result<String, String> {
    resolve_managed_project_name_typed(cfg, name).map_err(String::from)
}

pub(super) fn resolve_managed_project_name_typed(
    cfg: &Config,
    name: &str,
) -> Result<String, errors::PendingWorkError> {
    if cfg.projects.contains_key(name) {
        return Ok(name.to_string());
    }
    let names: Vec<&String> = cfg
        .projects
        .keys()
        .filter(|m| m.eq_ignore_ascii_case(name))
        .collect();
    let codes: Vec<&String> = cfg
        .prefixes
        .iter()
        .filter(|(_, code)| code.eq_ignore_ascii_case(name))
        .map(|(project, _)| project)
        .collect();
    // A unique name match wins over a code match; several matches at the
    // first non-empty tier are ambiguous instead of silently falling through.
    for tier in [names, codes] {
        match tier.as_slice() {
            [] => {}
            [only] => return Ok((*only).clone()),
            several => {
                return Err(errors::PendingWorkError::AmbiguousManagedProject {
                    identifier: name.to_string(),
                    matches: several.iter().map(|m| (*m).clone()).collect(),
                });
            }
        }
    }
    Err(errors::PendingWorkError::UnknownManagedProject {
        identifier: name.to_string(),
        known: cfg.projects.keys().cloned().collect(),
    })
}

/// Resolves a project identifier and requires a configured repository path.
pub(super) fn resolve_project_repo(
    cfg: &Config,
    raw: &str,
) -> Result<(String, String), errors::PendingWorkError> {
    let project = resolve_managed_project_name_typed(cfg, raw)?;
    let repo = cfg
        .projects
        .get(&project)
        .map_or("", std::string::String::as_str);
    if repo.trim().is_empty() {
        return Err(errors::PendingWorkError::ProjectNotMappedToRepo { project });
    }
    Ok((project, repo.to_string()))
}

/// Returns whether the ID is linked as open; checked and unknown IDs return false.
///
/// # Errors
///
/// Returns an error when the backing store query fails.
pub fn is_item_open(
    store: &impl AppDbStore<PendingWorkItem>,
    projects: &ProjectRegistry,
    id: &str,
) -> Result<bool, String> {
    is_item_open_typed(store, projects, id).map_err(String::from)
}

pub(super) fn is_item_open_typed(
    store: &impl AppDbStore<PendingWorkItem>,
    projects: &ProjectRegistry,
    id: &str,
) -> Result<bool, errors::PendingWorkError> {
    match pwf_application::pending_work::find::execute(
        &FindPendingWork { id: id.to_string() },
        store,
        projects,
    ) {
        Ok(_) => Ok(true),
        Err(FindPendingWorkError::ItemNotFound { .. }) => Ok(false),
        Err(other) => Err(map_find_error(other)),
    }
}

/// Finds an open item using case-insensitive ID matching.
pub(super) fn find_pending_item(
    store: &impl AppDbStore<PendingWorkItem>,
    projects: &ProjectRegistry,
    id: &str,
) -> Result<Item, errors::PendingWorkError> {
    pwf_application::pending_work::find::execute(
        &FindPendingWork { id: id.to_string() },
        store,
        projects,
    )
    .map(Into::into)
    .map_err(map_find_error)
}

/// Preserves legacy text while mapping Obsidian read failures to typed engine errors.
fn map_find_error(error: FindPendingWorkError) -> errors::PendingWorkError {
    match error {
        FindPendingWorkError::ItemNotFound { id } => errors::PendingWorkError::ItemNotFound { id },
        FindPendingWorkError::AmbiguousId { id } => errors::PendingWorkError::AmbiguousId { id },
        FindPendingWorkError::ReadStore(source) => map_store_read_error(source.as_ref()),
        other @ FindPendingWorkError::UnknownPrefix { .. } => {
            errors::PendingWorkError::ApplicationRead(other.to_string())
        }
    }
}

fn map_store_read_error(
    error: &(dyn std::error::Error + Send + Sync + 'static),
) -> errors::PendingWorkError {
    match error.downcast_ref::<ObsidianStoreError>() {
        Some(ObsidianStoreError::ItemNotFound { id }) => {
            errors::PendingWorkError::ItemNotFound { id: id.clone() }
        }
        Some(ObsidianStoreError::AmbiguousId { id }) => {
            errors::PendingWorkError::AmbiguousId { id: id.clone() }
        }
        Some(ObsidianStoreError::NotesDirectoryNotFound { path }) => {
            errors::PendingWorkError::NotesDirectoryNotFound { path: path.clone() }
        }
        _ => errors::PendingWorkError::ApplicationRead(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::{assert_matches, collections::BTreeMap};

    use super::*;
    use crate::cli::EngineArgs;

    fn cfg() -> Config {
        let json = r#"{
            "notesDir": "/n",
            "projects": { "git-tools": "/r/gt", "glep-shimeji": "/r/gs", "alpha": "/r/a", "beta": "/r/b" },
            "prefixes": { "git-tools": "GTL", "glep-shimeji": "GLP", "alpha": "BE", "beta": "BB" }
        }"#;
        crate::config::from_json(json, None).unwrap()
    }

    #[test]
    fn prefix_code_resolves_to_project() {
        let c = cfg();
        assert_eq!(
            resolve_managed_project_name(&c, "gtl").unwrap(),
            "git-tools"
        );
    }

    #[test]
    fn prefix_code_is_case_insensitive() {
        let c = cfg();
        for code in ["GTL", "Gtl", "gtl"] {
            assert_eq!(resolve_managed_project_name(&c, code).unwrap(), "git-tools");
        }
    }

    #[test]
    fn exact_name_wins_over_code() {
        let c = cfg();
        assert_eq!(
            resolve_managed_project_name(&c, "git-tools").unwrap(),
            "git-tools"
        );
    }

    #[test]
    fn code_resolves_even_when_a_project_name_extends_it() {
        let c = cfg();
        assert_eq!(resolve_managed_project_name(&c, "be").unwrap(), "alpha");
    }

    #[test]
    fn name_prefix_abbreviations_are_rejected_as_unknown() {
        let c = cfg();
        for abbreviation in ["glep", "git", "g"] {
            assert_matches!(
                resolve_managed_project_name_typed(&c, abbreviation).unwrap_err(),
                errors::PendingWorkError::UnknownManagedProject { .. },
                "'{abbreviation}' must not abbreviate a project name"
            );
        }
    }

    #[test]
    fn unknown_identifier_errors() {
        let c = cfg();
        assert!(resolve_managed_project_name(&c, "zzz").is_err());
    }

    #[test]
    fn missing_config_path_returns_typed_error_with_legacy_display() {
        let args = EngineArgs::default();

        let err = resolve_config_path_with_default(&args, || None).unwrap_err();

        assert_matches!(err, errors::PendingWorkError::MissingConfigPath);
        assert_eq!(err.to_string(), "missing --config-path");
    }

    #[test]
    fn load_config_returns_typed_config_error_with_legacy_display() {
        let guard = tempfile::tempdir().unwrap();
        let missing_config = guard.path().join("missing_config.json");
        assert!(!missing_config.exists());
        let args = EngineArgs {
            config_path: Some(missing_config.to_string_lossy().into_owned()),
            ..EngineArgs::default()
        };

        let err = load_config(&args).unwrap_err();

        assert_matches!(err, errors::PendingWorkError::Config(_));
        assert!(std::error::Error::source(&err).is_some());
        assert_eq!(
            err.to_string(),
            format!(
                "Pending work config not found: {}",
                missing_config.display()
            )
        );
    }

    #[test]
    fn duplicated_code_returns_typed_ambiguity_error_with_legacy_display() {
        let json = r#"{
            "notesDir": "/n",
            "projects": { "git-tools": "/r/gt", "glep-shimeji": "/r/gs" },
            "prefixes": { "git-tools": "DUP", "glep-shimeji": "DUP" }
        }"#;
        let c = crate::config::from_json(json, None).unwrap();

        let err = resolve_managed_project_name_typed(&c, "dup").unwrap_err();

        assert_matches!(
            err,
            errors::PendingWorkError::AmbiguousManagedProject {
                ref identifier,
                ref matches
            } if identifier == "dup"
                && matches == &vec!["git-tools".to_string(), "glep-shimeji".to_string()]
        );
        assert_eq!(
            err.to_string(),
            "'dup' is ambiguous. Managed project identifiers matching it: git-tools, glep-shimeji."
        );
        assert_eq!(
            resolve_managed_project_name(&c, "dup").unwrap_err(),
            "'dup' is ambiguous. Managed project identifiers matching it: git-tools, glep-shimeji."
        );
    }

    #[test]
    fn unknown_identifier_returns_typed_error_with_legacy_display() {
        let c = cfg();

        let err = resolve_managed_project_name_typed(&c, "zzz").unwrap_err();

        assert_matches!(
            err,
            errors::PendingWorkError::UnknownManagedProject {
                ref identifier,
                ref known
            } if identifier == "zzz"
                && known == &vec![
                    "alpha".to_string(),
                    "beta".to_string(),
                    "git-tools".to_string(),
                    "glep-shimeji".to_string()
                ]
        );
        assert_eq!(
            err.to_string(),
            "Unknown managed project identifier: zzz\nManaged project identifiers: alpha, beta, git-tools, glep-shimeji"
        );
        assert_eq!(
            resolve_managed_project_name(&c, "zzz").unwrap_err(),
            "Unknown managed project identifier: zzz\nManaged project identifiers: alpha, beta, git-tools, glep-shimeji"
        );
    }

    #[test]
    fn blank_repo_mapping_returns_typed_error_with_legacy_display() {
        let mut c = cfg();
        c.projects.insert("empty".to_string(), "  ".to_string());

        let err = resolve_project_repo(&c, "empty").unwrap_err();

        assert_matches!(
            err,
            errors::PendingWorkError::ProjectNotMappedToRepo { ref project }
                if project == "empty"
        );
        assert_eq!(
            err.to_string(),
            "Project 'empty' is not mapped to a repo in config/pending-work.json."
        );
    }

    #[test]
    fn is_item_open_returns_typed_read_error_with_legacy_display() {
        let mut projects = BTreeMap::new();
        projects.insert("pwf".to_string(), "/repo/pwf".to_string());
        let mut prefixes = BTreeMap::new();
        prefixes.insert("pwf".to_string(), "PWF".to_string());
        let c = Config {
            notes_dir: "/path/that/does/not/exist".to_string(),
            projects,
            prefixes,
            work_prefix: "WRK".to_string(),
            notes_dir_overrides: BTreeMap::new(),
        };

        let store = super::super::store_for(&c);
        let registry = super::super::run::project_registry(&c);
        let err = is_item_open_typed(&store, &registry, "PWF-0001").unwrap_err();

        assert_matches!(
            err,
            errors::PendingWorkError::NotesDirectoryNotFound { ref path }
                if path == "/path/that/does/not/exist"
        );
        assert_eq!(
            is_item_open(&store, &registry, "PWF-0001").unwrap_err(),
            "Notes directory not found: /path/that/does/not/exist"
        );
    }

    #[test]
    fn find_pending_item_not_found_returns_typed_error_with_legacy_display() {
        let dir = tempfile::tempdir().unwrap();
        let mut projects = BTreeMap::new();
        projects.insert("pwf".to_string(), "/repo/pwf".to_string());
        let mut prefixes = BTreeMap::new();
        prefixes.insert("pwf".to_string(), "PWF".to_string());
        let c = Config {
            notes_dir: dir.path().to_string_lossy().into_owned(),
            projects,
            prefixes,
            work_prefix: "WRK".to_string(),
            notes_dir_overrides: BTreeMap::new(),
        };

        let store = super::super::store_for(&c);
        let registry = super::super::run::project_registry(&c);
        let err = find_pending_item(&store, &registry, "PWF-9999").unwrap_err();

        assert_matches!(
            err,
            errors::PendingWorkError::ItemNotFound { ref id } if id == "PWF-9999"
        );
        assert_eq!(
            err.to_string(),
            "Open pending-work item not found: PWF-9999"
        );
        let as_string: String = err.into();
        assert_eq!(as_string, "Open pending-work item not found: PWF-9999");
    }
}
