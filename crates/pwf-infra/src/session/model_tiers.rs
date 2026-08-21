//! Loads raw model-tier entries from the configured TOML catalog.

use std::{collections::BTreeMap, path::PathBuf};

use pwf_application::contract::task::session::{ModelTier, ModelTierLookup};
use pwf_models::task::EffortTier;
use serde::Deserialize;
use thiserror::Error;

use super::ProcessEnvironment;

const MODEL_TIERS_ENV: &str = "PWF_MODEL_TIERS";

/// Reports failures resolving or loading the model-tier catalog.
#[derive(Debug, Error)]
pub enum ModelTiersError {
    #[error("could not resolve a model-tiers config path (set PWF_MODEL_TIERS)")]
    PathUnresolvable,
    #[error("model-tiers config not found: {}", .0.display())]
    NotFound(PathBuf),
    #[error("model-tiers config parse error: {0}")]
    Parse(
        #[from]
        #[source]
        toml::de::Error,
    ),
}

pub(super) fn tier(
    effort: EffortTier,
    environment: &ProcessEnvironment,
) -> Result<ModelTierLookup, ModelTiersError> {
    let catalog = default_model_tiers_path(environment).ok_or(ModelTiersError::PathUnresolvable)?;
    tier_at(&catalog, effort)
}

#[derive(Debug, Deserialize)]
struct RawModelTiers {
    tiers: BTreeMap<String, TierEntry>,
}

#[derive(Debug, Deserialize)]
struct TierEntry {
    claude_model: Option<String>,
}

fn default_model_tiers_path(environment: &ProcessEnvironment) -> Option<PathBuf> {
    resolve_model_tiers_path(
        environment.value(MODEL_TIERS_ENV).map(PathBuf::from),
        std::env::current_exe().ok(),
    )
}

fn resolve_model_tiers_path(
    env_path: Option<PathBuf>,
    executable: Option<PathBuf>,
) -> Option<PathBuf> {
    if let Some(path) = env_path
        && !path.as_os_str().is_empty()
    {
        return Some(path);
    }
    let executable = executable?;
    let directory = executable.parent()?;
    Some(
        directory
            .join("..")
            .join("..")
            .join("config")
            .join("model-tiers.toml"),
    )
}

fn tier_at(path: &std::path::Path, effort: EffortTier) -> Result<ModelTierLookup, ModelTiersError> {
    let text =
        std::fs::read_to_string(path).map_err(|_| ModelTiersError::NotFound(path.to_path_buf()))?;
    let raw: RawModelTiers = toml::from_str(&text)?;
    let tier = raw.tiers.get(effort.as_ref()).map(|entry| ModelTier {
        claude_model: entry.claude_model.clone(),
    });
    Ok(ModelTierLookup {
        catalog: path.to_path_buf(),
        tier,
    })
}

#[cfg(test)]
mod tests {
    use std::assert_matches;

    use super::*;

    fn stage(contents: &str) -> (tempfile::TempDir, PathBuf) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("model-tiers.toml");
        std::fs::write(&path, contents).unwrap();
        (directory, path)
    }

    #[test]
    fn environment_override_is_the_catalog_path_verbatim() {
        let path = resolve_model_tiers_path(
            Some(PathBuf::from("/config/model-tiers.toml")),
            Some(PathBuf::from("/bin/pwf")),
        );

        assert_eq!(
            path.as_deref(),
            Some(std::path::Path::new("/config/model-tiers.toml"))
        );
    }

    #[test]
    fn empty_environment_override_falls_back_to_binary_relative_path() {
        let executable = PathBuf::from("tools").join("pwf").join("bin").join("pwf");
        let expected = executable
            .parent()
            .unwrap()
            .join("..")
            .join("..")
            .join("config")
            .join("model-tiers.toml");

        let path = resolve_model_tiers_path(Some(PathBuf::new()), Some(executable));

        assert_eq!(path, Some(expected));
    }

    #[test]
    fn absent_sources_are_unresolvable() {
        assert_eq!(resolve_model_tiers_path(None, None), None);
    }

    #[test]
    fn configured_tier_retains_catalog_provenance() {
        let (_directory, path) = stage("[tiers.high]\nclaude_model = \"sonnet\"\n");

        let lookup = tier_at(&path, EffortTier::High).unwrap();

        assert_eq!(lookup.catalog, path);
        assert_eq!(
            lookup.tier,
            Some(ModelTier {
                claude_model: Some("sonnet".to_string())
            })
        );
    }

    #[test]
    fn empty_config_value_remains_raw_data() {
        let (_directory, path) = stage("[tiers.medium]\nclaude_model = \"\"\n");

        let lookup = tier_at(&path, EffortTier::Medium).unwrap();

        assert_eq!(
            lookup.tier,
            Some(ModelTier {
                claude_model: Some(String::new())
            })
        );
    }

    #[test]
    fn absent_tier_is_a_successful_empty_lookup() {
        let (_directory, path) = stage("[tiers.low]\nclaude_model = \"sonnet\"\n");

        let lookup = tier_at(&path, EffortTier::Medium).unwrap();

        assert_eq!(lookup.catalog, path);
        assert_eq!(lookup.tier, None);
    }

    #[test]
    fn absent_claude_model_remains_raw_data() {
        let (_directory, path) = stage("[tiers.low]\n");

        let lookup = tier_at(&path, EffortTier::Low).unwrap();

        assert_eq!(lookup.tier, Some(ModelTier { claude_model: None }));
    }

    #[test]
    fn missing_file_is_not_found() {
        let directory = tempfile::tempdir().unwrap();
        let missing = directory.path().join("does-not-exist.toml");

        let error = tier_at(&missing, EffortTier::Low).unwrap_err();

        assert_matches!(error, ModelTiersError::NotFound(_));
    }

    #[test]
    fn malformed_toml_is_parse_error() {
        let (_directory, path) = stage("this is not toml [[[");

        let error = tier_at(&path, EffortTier::Low).unwrap_err();

        assert_matches!(error, ModelTiersError::Parse(_));
    }
}
