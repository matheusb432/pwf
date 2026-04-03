// Config loading + read-side store: project-name resolution and item enumeration.

use super::errors;
use super::model::Item;
use super::naming::project_index_path;
use super::parse::get_project_tasks;
use crate::config::Config;
use std::path::Path;

pub(super) fn load_config(args: &crate::cli::Args) -> Result<Config, String> {
    let cfg_path = args
        .config_path
        .clone()
        .or_else(crate::config::default_config_path)
        .ok_or("missing --config-path")?;
    Ok(crate::config::load(&cfg_path, args.notes_dir.as_deref())?)
}

/// Resolve a project name against the managed list: exact, case-insensitive, unique prefix.
///
/// `cfg.projects` is a `BTreeMap`, so its keys already iterate in sorted order —
/// there's no need to materialize + sort a `Vec` to match or to build the hint
/// lists. Exact match is an O(log n) tree lookup; the fuzzy fallbacks scan keys.
pub fn resolve_managed_project_name(cfg: &Config, name: &str) -> Result<String, String> {
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
        let hint = pfx
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!(
            "'{name}' is ambiguous. Managed project identifiers matching it: {hint}."
        ));
    }
    let hint = cfg.projects.keys().cloned().collect::<Vec<_>>().join(", ");
    Err(format!(
        "Unknown managed project identifier: {name}\nManaged project identifiers: {hint}"
    ))
}

/// Resolve a managed project name together with its configured repo path, erroring
/// if the name is unknown or maps to no repo. Used by the `new`/`add` paths, which
/// need the (project, repo) pair.
pub(super) fn resolve_project_repo(cfg: &Config, raw: &str) -> Result<(String, String), String> {
    let project = resolve_managed_project_name(cfg, raw)?;
    let repo = cfg.projects.get(&project).map(|s| s.as_str()).unwrap_or("");
    if repo.trim().is_empty() {
        return Err(errors::not_mapped_to_repo(&project));
    }
    Ok((project, repo.to_string()))
}

/// Enumerate all pending-work items across managed projects (or one project).
pub(super) fn get_pending_work(
    cfg: &Config,
    only_project: Option<&str>,
) -> Result<Vec<Item>, String> {
    if !Path::new(&cfg.notes_dir).exists() {
        return Err(format!("Notes directory not found: {}", cfg.notes_dir));
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
    Ok(get_pending_work(cfg, None)?.iter().any(|i| i.id == id))
}

/// Finds pending item by cli input id.
/// Applies case insensitive search so "cfg-0001" matches to "CFG-0001".
pub(super) fn find_pending_item(cfg: &Config, id: &str) -> Result<Item, String> {
    let items = get_pending_work(cfg, None)?;
    let selected: Vec<&Item> = items
        .iter()
        .filter(|i| i.id.eq_ignore_ascii_case(id))
        .collect();
    match selected.len() {
        0 => Err(format!("Open pending-work item not found: {id}")),
        1 => Ok(selected[0].clone()),
        _ => Err(format!("Pending-work id is ambiguous: {id}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
