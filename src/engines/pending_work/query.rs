// Config loading + read-side store: project-name resolution and item enumeration.

use std::path::{Path, PathBuf};

use super::{
    errors,
    model::Item,
    naming::{project_archive_dir, project_dir, project_index_path},
    parse::get_project_tasks,
};
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
    let repo = cfg.projects.get(&project).map(|s| s.as_str()).unwrap_or("");
    if repo.trim().is_empty() {
        return Err(errors::PendingWorkError::ProjectNotMappedToRepo { project });
    }
    Ok((project, repo.to_string()))
}

/// Enumerate all pending-work items across managed projects (or one project).
pub(super) fn get_pending_work(
    cfg: &Config,
    only_project: Option<&str>,
) -> Result<Vec<Item>, errors::PendingWorkError> {
    if !Path::new(&cfg.notes_dir).exists() {
        return Err(errors::PendingWorkError::NotesDirectoryNotFound {
            path: cfg.notes_dir.clone(),
        });
    }
    let project_names: Vec<String> = if let Some(p) = only_project {
        vec![p.to_string()]
    } else {
        let mut v: Vec<String> = cfg.projects.keys().cloned().collect();
        v.sort();
        v
    };
    let mut items: Vec<Item> = Vec::new();
    for project in &project_names {
        let index = project_index_path(cfg.notes_dir_for(project), project);
        if !index.exists() {
            continue;
        }
        let repo = cfg.projects.get(project).map(|s| s.as_str());
        items.extend(get_project_tasks(project, repo, &index));
    }
    Ok(items)
}

/// Whether `id` is still an open pending-work item (linked in a project index).
/// Already-checked and unknown ids both report not-open.
pub fn is_item_open(cfg: &Config, id: &str) -> Result<bool, String> {
    is_item_open_typed(cfg, id).map_err(String::from)
}

pub(super) fn is_item_open_typed(cfg: &Config, id: &str) -> Result<bool, errors::PendingWorkError> {
    Ok(get_pending_work(cfg, None)?.iter().any(|i| i.id == id))
}

/// Finds pending item by cli input id.
/// Applies case insensitive search so "cfg-0001" matches to "CFG-0001".
pub(super) fn find_pending_item(cfg: &Config, id: &str) -> Result<Item, errors::PendingWorkError> {
    let items = get_pending_work(cfg, None)?;
    let selected: Vec<&Item> = items
        .iter()
        .filter(|i| i.id.eq_ignore_ascii_case(id))
        .collect();
    match selected.len() {
        0 => Err(errors::PendingWorkError::ItemNotFound { id: id.to_string() }),
        1 => Ok(selected[0].clone()),
        _ => Err(errors::PendingWorkError::AmbiguousId { id: id.to_string() }),
    }
}

/// Locate an item's note file by id (case-insensitive) outside the active index.
/// Done/cancelled items are skipped by the index parser (`- [x]` links), so they
/// are invisible to `find_pending_item`: a freshly-checked item still sits as
/// `<ID>.md` in the project dir, while one evicted past the done-queue cap is moved
/// to `_archive/`. `resolve` falls back here to show tasks regardless of status —
/// project dir first, then archive (PWF-0061).
pub(super) fn find_item_note_file(cfg: &Config, id: &str) -> Option<PathBuf> {
    find_item_note_with_project(cfg, id).map(|(_, path)| path)
}

/// Like [`find_item_note_file`], but also returns the managed project the note
/// belongs to — needed when a verb (e.g. `reopen`) must rewrite the project index
/// and possibly move the note out of `_archive` back into the project dir.
pub(super) fn find_item_note_with_project(cfg: &Config, id: &str) -> Option<(String, PathBuf)> {
    for project in cfg.projects.keys() {
        let base = cfg.notes_dir_for(project);
        for dir in [
            project_dir(base, project),
            project_archive_dir(base, project),
        ] {
            if let Some(path) = scan_dir_for_item_note(&dir, id) {
                return Some((project.clone(), path));
            }
        }
    }
    None
}

/// First `<dir>/*.md` whose stem matches `id` case-insensitively. The index file
/// (`<project>.md`) and sibling items have different stems, so only `<ID>.md` hits.
fn scan_dir_for_item_note(dir: &Path, id: &str) -> Option<PathBuf> {
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        if path
            .file_stem()
            .and_then(|s| s.to_str())
            .is_some_and(|stem| stem.eq_ignore_ascii_case(id))
        {
            return Some(path);
        }
    }
    None
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
        let c = Config {
            notes_dir: "/path/that/does/not/exist".to_string(),
            projects: BTreeMap::new(),
            prefixes: BTreeMap::new(),
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
    fn missing_notes_dir_returns_typed_error_with_legacy_display() {
        let c = Config {
            notes_dir: "/path/that/does/not/exist".to_string(),
            projects: BTreeMap::new(),
            prefixes: BTreeMap::new(),
            work_prefix: "WRK".to_string(),
            notes_dir_overrides: BTreeMap::new(),
        };

        let err = get_pending_work(&c, None).unwrap_err();

        assert_matches!(
            err,
            errors::PendingWorkError::NotesDirectoryNotFound { ref path }
                if path == "/path/that/does/not/exist"
        );
        assert_eq!(
            err.to_string(),
            "Notes directory not found: /path/that/does/not/exist"
        );
    }

    #[test]
    fn find_pending_item_not_found_returns_typed_error_with_legacy_display() {
        let dir = tempfile::tempdir().unwrap();
        let c = Config {
            notes_dir: dir.path().to_string_lossy().into_owned(),
            projects: BTreeMap::new(),
            prefixes: BTreeMap::new(),
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

    #[test]
    fn find_pending_item_ambiguous_returns_typed_error_with_legacy_display() {
        let dir = tempfile::tempdir().unwrap();
        let alpha = dir.path().join("alpha");
        let beta = dir.path().join("beta");
        std::fs::create_dir_all(&alpha).unwrap();
        std::fs::create_dir_all(&beta).unwrap();
        for project_dir in [&alpha, &beta] {
            std::fs::write(
                project_dir.join("PWF-0001.md"),
                "---\nstatus: active\ntitle: duplicate\nproject: pwf\ncreated: 2026-01-01\n---\n\nreal prompt\n",
            )
            .unwrap();
        }
        std::fs::write(alpha.join("alpha.md"), "- [ ] [[PWF-0001]]\n").unwrap();
        std::fs::write(beta.join("beta.md"), "- [ ] [[PWF-0001]]\n").unwrap();
        let mut projects = BTreeMap::new();
        projects.insert("alpha".to_string(), "/repo/alpha".to_string());
        projects.insert("beta".to_string(), "/repo/beta".to_string());
        let c = Config {
            notes_dir: dir.path().to_string_lossy().into_owned(),
            projects,
            prefixes: BTreeMap::new(),
            work_prefix: "WRK".to_string(),
            notes_dir_overrides: BTreeMap::new(),
        };

        let err = find_pending_item(&c, "pwf-0001").unwrap_err();

        assert_matches!(
            err,
            errors::PendingWorkError::AmbiguousId { ref id } if id == "pwf-0001"
        );
        assert_eq!(err.to_string(), "Pending-work id is ambiguous: pwf-0001");
    }
}
