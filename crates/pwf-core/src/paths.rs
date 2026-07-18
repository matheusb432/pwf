//! Project path construction and prefix lookup over a resolved [`Config`].

use std::path::{Path, PathBuf};

use crate::config::Config;

/// Returns the project's `<notes_dir>/<name>` directory.
pub fn project_dir(notes_dir: &str, name: &str) -> PathBuf {
    Path::new(notes_dir).join(name)
}

/// Returns the project's `<notes_dir>/<name>/<name>.md` index path.
pub fn project_index_path(notes_dir: &str, name: &str) -> PathBuf {
    project_dir(notes_dir, name).join(format!("{name}.md"))
}

/// Returns the configured id prefix, excluding missing and blank values.
pub fn project_key<'a>(cfg: &'a Config, name: &str) -> Option<&'a str> {
    cfg.prefixes
        .get(name)
        .map(String::as_str)
        .filter(|key| !key.trim().is_empty())
}

/// Resolves a raw project token to its configured canonical name.
///
/// Matches, in order: exact name, unique case-insensitive name, unique
/// case-insensitive id code (`prefixes` value). Blank or ambiguous tokens
/// resolve to `None`.
pub fn resolve_project_name<'a>(cfg: &'a Config, raw: &str) -> Option<&'a str> {
    if raw.trim().is_empty() {
        return None;
    }
    if let Some((name, _)) = cfg.prefixes.get_key_value(raw) {
        return Some(name.as_str());
    }
    unique(
        cfg.prefixes
            .keys()
            .filter(|name| name.eq_ignore_ascii_case(raw)),
    )
    .or_else(|| {
        unique(
            cfg.prefixes
                .iter()
                .filter(|(_, code)| code.eq_ignore_ascii_case(raw))
                .map(|(name, _)| name),
        )
    })
    .map(String::as_str)
}

fn unique<'a>(mut candidates: impl Iterator<Item = &'a String>) -> Option<&'a String> {
    let first = candidates.next()?;
    candidates.next().is_none().then_some(first)
}

#[cfg(test)]
mod resolve_project_name_tests {
    use super::*;

    fn cfg() -> Config {
        crate::config::from_json(
            r#"{
                "notesDir": "/notes",
                "projects": { "pwf": "/repo/pwf", "glep-shimeji": "/repo/glep" },
                "prefixes": { "pwf": "PWF", "glep-shimeji": "GLP" }
            }"#,
            None,
        )
        .unwrap()
    }

    #[test]
    fn exact_name_resolves() {
        assert_eq!(resolve_project_name(&cfg(), "pwf"), Some("pwf"));
    }

    #[test]
    fn name_resolves_case_insensitively() {
        assert_eq!(resolve_project_name(&cfg(), "PWF"), Some("pwf"));
        assert_eq!(
            resolve_project_name(&cfg(), "Glep-Shimeji"),
            Some("glep-shimeji")
        );
    }

    #[test]
    fn id_code_resolves_case_insensitively() {
        assert_eq!(resolve_project_name(&cfg(), "GLP"), Some("glep-shimeji"));
        assert_eq!(resolve_project_name(&cfg(), "glp"), Some("glep-shimeji"));
    }

    #[test]
    fn unknown_and_blank_tokens_resolve_to_none() {
        assert_eq!(resolve_project_name(&cfg(), "nope"), None);
        assert_eq!(resolve_project_name(&cfg(), ""), None);
        assert_eq!(resolve_project_name(&cfg(), "   "), None);
    }

    #[test]
    fn name_match_wins_over_code_match() {
        let cfg = crate::config::from_json(
            r#"{
                "notesDir": "/notes",
                "projects": { "glp": "/repo/a", "glep-shimeji": "/repo/b" },
                "prefixes": { "glp": "AAA", "glep-shimeji": "GLP" }
            }"#,
            None,
        )
        .unwrap();
        assert_eq!(resolve_project_name(&cfg, "GLP"), Some("glp"));
    }

    #[test]
    fn ambiguous_code_match_resolves_to_none() {
        let cfg = crate::config::from_json(
            r#"{
                "notesDir": "/notes",
                "projects": { "a": "/repo/a", "b": "/repo/b" },
                "prefixes": { "a": "DUP", "b": "DUP" }
            }"#,
            None,
        )
        .unwrap();
        assert_eq!(resolve_project_name(&cfg, "dup"), None);
    }
}
