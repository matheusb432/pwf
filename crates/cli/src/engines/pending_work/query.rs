// Config loading + read-side store: project-name resolution and item enumeration.

use pwf_application::PendingWorkReadStore;
use pwf_infra::obsidian::{ObsidianPendingWorkStore, ObsidianPendingWorkStoreError};

use super::{errors, model::Item};
use crate::config::Config;

pub(super) fn load_config(args: &crate::cli::Args) -> Result<Config, errors::PendingWorkError> {
    let cfg_path = resolve_config_path_with_default(args, crate::config::default_config_path)?;
    Ok(crate::config::load(&cfg_path, args.notes_dir.as_deref())?)
}

fn resolve_config_path_with_default(
    args: &crate::cli::Args,
    default_config_path: impl FnOnce() -> Option<String>,
) -> Result<String, errors::PendingWorkError> {
    args.config_path
        .clone()
        .or_else(default_config_path)
        .ok_or(errors::PendingWorkError::MissingConfigPath)
}

/// Resolve a project name against the managed list: exact, case-insensitive, unique prefix.
///
/// `cfg.projects` is a `BTreeMap`, so its keys already iterate in sorted order —
/// there's no need to materialize + sort a `Vec` to match or to build the hint
/// lists. Exact match is an O(log n) tree lookup; the fuzzy fallbacks scan keys.
pub fn resolve_managed_project_name(cfg: &Config, name: &str) -> Result<String, String> {
    resolve_managed_project_name_typed(cfg, name).map_err(String::from)
}

pub(super) fn resolve_managed_project_name_typed(
    cfg: &Config,
    name: &str,
) -> Result<String, errors::PendingWorkError> {
    // Exact match.
    if cfg.projects.contains_key(name) {
        return Ok(name.to_string());
    }
    // Case-insensitive exact: take it only if it's the unique winner.
    let mut ci = cfg.projects.keys().filter(|m| m.eq_ignore_ascii_case(name));
    if let Some(first) = ci.next()
        && ci.next().is_none()
    {
        return Ok(first.clone());
    }
    // Exact prefix-code match (case-insensitive). `cfg.prefixes` maps project
    // name -> CODE, so match on the value and return the key. Codes are exact
    // identifiers, so they rank above the fuzzy name-prefix fallback below.
    let mut code = cfg
        .prefixes
        .iter()
        .filter(|(_, c)| c.eq_ignore_ascii_case(name))
        .map(|(proj, _)| proj);
    if let Some(first) = code.next()
        && code.next().is_none()
    {
        return Ok(first.clone());
    }
    // Unique prefix (case-insensitive).
    let lower = name.to_ascii_lowercase();
    let pfx: Vec<&String> = cfg
        .projects
        .keys()
        .filter(|m| m.to_ascii_lowercase().starts_with(&lower))
        .collect();
    if pfx.len() == 1 {
        return Ok(pfx[0].clone());
    }
    if pfx.len() > 1 {
        return Err(errors::PendingWorkError::AmbiguousManagedProject {
            identifier: name.to_string(),
            matches: pfx.into_iter().cloned().collect(),
        });
    }
    Err(errors::PendingWorkError::UnknownManagedProject {
        identifier: name.to_string(),
        known: cfg.projects.keys().cloned().collect(),
    })
}

/// Resolve a managed project name together with its configured repo path, erroring
/// if the name is unknown or maps to no repo. Used by the `new`/`add` paths, which
/// need the (project, repo) pair.
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

/// Whether `id` is still an open pending-work item (linked in a project index).
/// Already-checked and unknown ids both report not-open.
pub fn is_item_open(cfg: &Config, id: &str) -> Result<bool, String> {
    is_item_open_typed(cfg, id).map_err(String::from)
}

pub(super) fn is_item_open_typed(cfg: &Config, id: &str) -> Result<bool, errors::PendingWorkError> {
    match ObsidianPendingWorkStore::new(cfg.clone()).open_item(id) {
        Ok(_) => Ok(true),
        Err(ObsidianPendingWorkStoreError::ItemNotFound { .. }) => Ok(false),
        Err(error) => Err(map_store_read_error(error)),
    }
}

/// Finds pending item by cli input id.
/// Applies case insensitive search so "cfg-0001" matches to "CFG-0001".
pub(super) fn find_pending_item(cfg: &Config, id: &str) -> Result<Item, errors::PendingWorkError> {
    ObsidianPendingWorkStore::new(cfg.clone())
        .open_item(id)
        .map(Into::into)
        .map_err(map_store_read_error)
}

fn map_store_read_error(error: ObsidianPendingWorkStoreError) -> errors::PendingWorkError {
    match error {
        ObsidianPendingWorkStoreError::ItemNotFound { id } => {
            errors::PendingWorkError::ItemNotFound { id }
        }
        ObsidianPendingWorkStoreError::AmbiguousId { id } => {
            errors::PendingWorkError::AmbiguousId { id }
        }
        ObsidianPendingWorkStoreError::NotesDirectoryNotFound { path } => {
            errors::PendingWorkError::NotesDirectoryNotFound { path }
        }
        other => errors::PendingWorkError::ApplicationRead(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::{assert_matches, collections::BTreeMap};

    use super::*;
    use crate::cli::Args;

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
    fn code_wins_over_name_prefix() {
        // "be" is alpha's code AND a name-prefix of "beta"; the exact code wins.
        let c = cfg();
        assert_eq!(resolve_managed_project_name(&c, "be").unwrap(), "alpha");
    }

    #[test]
    fn unique_name_prefix_still_resolves() {
        let c = cfg();
        assert_eq!(
            resolve_managed_project_name(&c, "glep").unwrap(),
            "glep-shimeji"
        );
    }

    #[test]
    fn unknown_identifier_errors() {
        let c = cfg();
        assert!(resolve_managed_project_name(&c, "zzz").is_err());
    }

    #[test]
    fn missing_config_path_returns_typed_error_with_legacy_display() {
        let args = Args::default();

        let err = resolve_config_path_with_default(&args, || None).unwrap_err();

        assert_matches!(err, errors::PendingWorkError::MissingConfigPath);
        assert_eq!(err.to_string(), "missing --config-path");
    }

    #[test]
    fn load_config_returns_typed_config_error_with_legacy_display() {
        let guard = tempfile::tempdir().unwrap();
        let missing_config = guard.path().join("missing_config.json");
        assert!(!missing_config.exists());
        let args = Args {
            config_path: Some(missing_config.to_string_lossy().into_owned()),
            ..Args::default()
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
    fn ambiguous_identifier_returns_typed_error_with_legacy_display() {
        let c = cfg();

        let err = resolve_managed_project_name_typed(&c, "g").unwrap_err();

        assert_matches!(
            err,
            errors::PendingWorkError::AmbiguousManagedProject {
                ref identifier,
                ref matches
            } if identifier == "g"
                && matches == &vec!["git-tools".to_string(), "glep-shimeji".to_string()]
        );
        assert_eq!(
            err.to_string(),
            "'g' is ambiguous. Managed project identifiers matching it: git-tools, glep-shimeji."
        );
        assert_eq!(
            resolve_managed_project_name(&c, "g").unwrap_err(),
            "'g' is ambiguous. Managed project identifiers matching it: git-tools, glep-shimeji."
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

        let err = is_item_open_typed(&c, "PWF-0001").unwrap_err();

        assert_matches!(
            err,
            errors::PendingWorkError::NotesDirectoryNotFound { ref path }
                if path == "/path/that/does/not/exist"
        );
        assert_eq!(
            is_item_open(&c, "PWF-0001").unwrap_err(),
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

        let err = find_pending_item(&c, "PWF-9999").unwrap_err();

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
