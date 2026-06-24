use std::{collections::BTreeMap, path::Path};

use serde::Deserialize;
use thiserror::Error;

/// Errors at the config boundary. Display text is frozen to the pre-clap strings
/// so existing CLI diagnostics stay stable.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// The config file could not be read at the given path.
    #[error("Pending work config not found: {0}")]
    NotFound(String),
    /// The config JSON failed to parse.
    #[error("config parse error: {0}")]
    Parse(#[from] serde_json::Error),
}

// ? Engines return `Result<_, String>`; this lift lets `?` propagate a
// ? `ConfigError` as its exact Display text — no output change.
impl From<ConfigError> for String {
    fn from(e: ConfigError) -> Self {
        e.to_string()
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub notes_dir: String,
    pub projects: BTreeMap<String, String>,
    pub prefixes: BTreeMap<String, String>,
    pub work_prefix: String,
    pub notes_dir_overrides: BTreeMap<String, String>,
}

impl Config {
    /// Notes root for a project: per-project override if present, else the global notes dir.
    pub fn notes_dir_for(&self, project: &str) -> &str {
        self.notes_dir_overrides
            .get(project)
            .map(String::as_str)
            .unwrap_or(&self.notes_dir)
    }
}

#[derive(Deserialize)]
struct RawConfig {
    #[serde(rename = "notesDir")]
    notes_dir: Option<String>,
    #[serde(default)]
    projects: BTreeMap<String, String>,
    #[serde(default)]
    prefixes: BTreeMap<String, String>,
    #[serde(rename = "workPrefix")]
    work_prefix: Option<String>,
    #[serde(rename = "notesDirOverrides", default)]
    notes_dir_overrides: BTreeMap<String, String>,
}

/// Expand a leading `~` against `home`.
pub fn expand_user(path: &str, home: &str) -> String {
    if path == "~" {
        return home.to_string();
    }
    if let Some(rest) = path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\")) {
        // OS-native path join → backslash separators on Windows.
        return Path::new(home).join(rest).to_string_lossy().into_owned();
    }
    path.to_string()
}

fn home() -> String {
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default()
}

/// Parse config JSON. `notes_dir_override` is the `--notes-dir` flag (wins if set).
///
/// # Errors
///
/// Returns [`ConfigError::Parse`] if `json` is not valid config JSON.
pub fn from_json(json: &str, notes_dir_override: Option<&str>) -> Result<Config, ConfigError> {
    let raw: RawConfig = serde_json::from_str(json)?;
    let h = home();
    let notes_dir = match notes_dir_override {
        Some(o) => expand_user(o, &h),
        None => expand_user(&raw.notes_dir.unwrap_or_default(), &h),
    };
    let projects = raw
        .projects
        .into_iter()
        .map(|(k, v)| (k, expand_user(&v, &h)))
        .collect();
    let notes_dir_overrides = raw
        .notes_dir_overrides
        .into_iter()
        .map(|(k, v)| (k, expand_user(&v, &h)))
        .collect();
    Ok(Config {
        notes_dir,
        projects,
        prefixes: raw.prefixes,
        work_prefix: raw.work_prefix.unwrap_or_else(|| "WRK".to_string()),
        notes_dir_overrides,
    })
}

/// Load config from a file path (used by the CLI).
///
/// # Errors
///
/// Returns [`ConfigError::NotFound`] if `config_path` cannot be read, or
/// [`ConfigError::Parse`] if its contents are not valid config JSON.
pub fn load(config_path: &str, notes_dir_override: Option<&str>) -> Result<Config, ConfigError> {
    let json = std::fs::read_to_string(config_path)
        .map_err(|_| ConfigError::NotFound(config_path.to_string()))?;
    from_json(&json, notes_dir_override)
}

/// Default `--config-path` when none is supplied. Resolution order after the
/// explicit `--config` flag: the `PWF_CONFIG` env var (set by the scoop shim so
/// the global `pwf` finds the editable repo config), then the binary-relative
/// `<exe>/../../config/pending-work.json` (works for `target/release/pwf.exe`).
pub fn default_config_path() -> Option<String> {
    if let Ok(p) = std::env::var("PWF_CONFIG")
        && !p.is_empty()
    {
        return Some(p);
    }
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    Some(
        dir.join("..")
            .join("..")
            .join("config")
            .join("pending-work.json")
            .to_string_lossy()
            .into_owned(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_and_defaults_work_prefix() {
        let json = r#"{ "notesDir": "/n", "projects": { "a": "/r" }, "prefixes": { "a": "AAA" } }"#;
        let c = from_json(json, None).unwrap();
        assert_eq!(c.notes_dir, "/n");
        assert_eq!(c.projects.get("a").unwrap(), "/r");
        assert_eq!(c.prefixes.get("a").unwrap(), "AAA");
        assert_eq!(c.work_prefix, "WRK");
    }

    #[test]
    fn notes_dir_override_wins() {
        let json = r#"{ "notesDir": "/n", "projects": {}, "prefixes": {} }"#;
        let c = from_json(json, Some("/override")).unwrap();
        assert_eq!(c.notes_dir, "/override");
    }

    #[test]
    fn notes_dir_override_per_project() {
        let json = r#"{
            "notesDir": "/base",
            "projects": { "a": "/ra", "pwf": "/rpwf" },
            "prefixes": {},
            "notesDirOverrides": { "pwf": "/other" }
        }"#;
        let c = from_json(json, None).unwrap();
        assert_eq!(c.notes_dir_for("a"), "/base");
        assert_eq!(c.notes_dir_for("pwf"), "/other");
    }

    #[test]
    fn pwf_config_env_wins_over_relative() {
        // SAFETY: single-threaded test; restore after.
        unsafe { std::env::set_var("PWF_CONFIG", "/custom/pwf.json") };
        let got = default_config_path();
        unsafe { std::env::remove_var("PWF_CONFIG") };
        assert_eq!(got.as_deref(), Some("/custom/pwf.json"));
    }

    #[test]
    fn config_error_display_is_byte_identical_to_legacy_strings() {
        // PR2 invariant: typed errors must Display to the exact pre-refactor text
        // so stderr stays byte-identical for callers.
        let not_found = ConfigError::NotFound("/x/pending-work.json".to_string());
        assert_eq!(
            not_found.to_string(),
            "Pending work config not found: /x/pending-work.json"
        );
        let parse_err = from_json("{ not json", None).unwrap_err();
        assert!(
            parse_err.to_string().starts_with("config parse error: "),
            "got {parse_err}"
        );
    }

    #[test]
    fn config_error_converts_to_string_unchanged() {
        // Engines return Result<_, String>; `?` must lift ConfigError to the same text.
        let s: String = ConfigError::NotFound("/p".to_string()).into();
        assert_eq!(s, "Pending work config not found: /p");
    }

    #[test]
    fn expands_leading_tilde() {
        // OS-native join under home.
        let home = "C:\\Users\\me";
        assert_eq!(expand_user("~", home), home);
        let joined = expand_user("~/x", home);
        assert!(joined.starts_with(home), "got {joined}");
        assert!(joined.ends_with('x'), "got {joined}");
        assert_eq!(expand_user("/abs", home), "/abs");
    }
}
