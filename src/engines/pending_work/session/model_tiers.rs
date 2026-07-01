//! Loads `config/model-tiers.toml` and resolves an item's `effort` tier to a
//! per-provider model. Only `claude_model` is consumed today (codex is out of
//! scope for model selection); a future provider reads its own sibling key from
//! the same `[tiers.N]` table.

use std::collections::BTreeMap;

use serde::Deserialize;
use thiserror::Error;

use crate::engines::pending_work::{effort::EffortTier, errors::PendingWorkError, model::Item};

/// Errors loading or resolving `config/model-tiers.toml`.
#[derive(Debug, Error)]
pub(in crate::engines::pending_work) enum ModelTiersError {
    /// Neither `PWF_MODEL_TIERS` nor a binary-relative default could be resolved.
    #[error("could not resolve a model-tiers config path (set PWF_MODEL_TIERS)")]
    PathUnresolvable,
    /// The resolved config path could not be read.
    #[error("model-tiers config not found: {0}")]
    NotFound(String),
    /// The config file is not valid TOML.
    #[error("model-tiers config parse error: {0}")]
    Parse(
        #[from]
        #[source]
        toml::de::Error,
    ),
    /// The item's tier has no `[tiers.N]` entry at all.
    #[error("tier {tier} has no [tiers.{tier}] entry in {path}")]
    MissingTier {
        /// The effort tier that was looked up.
        tier: u8,
        /// The resolved model-tiers config path.
        path: String,
    },
    /// The tier exists but has no `claude_model` set.
    #[error("tier {tier} in {path} has no claude_model set")]
    MissingClaudeModel {
        /// The effort tier that was looked up.
        tier: u8,
        /// The resolved model-tiers config path.
        path: String,
    },
}

#[derive(Debug, Deserialize)]
struct RawModelTiers {
    tiers: BTreeMap<u8, TierEntry>,
}

#[derive(Debug, Deserialize)]
struct TierEntry {
    claude_model: Option<String>,
}

/// Resolution order for `config/model-tiers.toml`, mirroring
/// `pwf_core::config::default_config_path`: the `PWF_MODEL_TIERS` env var, then
/// the binary-relative `<exe>/../../config/model-tiers.toml`.
fn default_model_tiers_path() -> Option<String> {
    if let Ok(p) = std::env::var("PWF_MODEL_TIERS")
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
            .join("model-tiers.toml")
            .to_string_lossy()
            .into_owned(),
    )
}

fn load(path: &str) -> Result<RawModelTiers, ModelTiersError> {
    let text =
        std::fs::read_to_string(path).map_err(|_| ModelTiersError::NotFound(path.to_string()))?;
    Ok(toml::from_str(&text)?)
}

fn resolve_claude_model_at(path: &str, tier: EffortTier) -> Result<String, ModelTiersError> {
    let raw = load(path)?;
    let tier_num: u8 = tier.into();
    let entry = raw
        .tiers
        .get(&tier_num)
        .ok_or_else(|| ModelTiersError::MissingTier {
            tier: tier_num,
            path: path.to_string(),
        })?;
    entry
        .claude_model
        .clone()
        .ok_or_else(|| ModelTiersError::MissingClaudeModel {
            tier: tier_num,
            path: path.to_string(),
        })
}

/// If `agent` is Claude and `item` carries a valid `effort` tag, resolve the
/// Claude model to dispatch with via `config/model-tiers.toml`. `Ok(None)` when
/// not applicable (no `effort` tag, or a non-claude agent) — the common case,
/// zero behavior change. `Err` on a corrupted `effort:` value or a missing/
/// malformed/incomplete `config/model-tiers.toml`.
pub(in crate::engines::pending_work) fn resolve_claude_model(
    agent: crate::cli::Agent,
    item: &Item,
) -> Result<Option<String>, PendingWorkError> {
    if agent != crate::cli::Agent::Claude {
        return Ok(None);
    }
    let Some(raw) = item.effort.as_deref() else {
        return Ok(None);
    };
    let tier = EffortTier::parse(raw).ok_or_else(|| PendingWorkError::BadEffortValue {
        id: item.id.clone(),
        value: raw.to_string(),
    })?;
    let path = default_model_tiers_path().ok_or(ModelTiersError::PathUnresolvable)?;
    Ok(Some(resolve_claude_model_at(&path, tier)?))
}

/// `resolve_claude_model` mapped into the `Option<Result<String, String>>` shape
/// `verify_text_with_probe` expects: `None` when not applicable, `Some(Ok(model))`
/// on a resolved model, `Some(Err(display))` on a resolution error. Shared by
/// `pwf verify` and the bare-word `verify`/`v` route alias, which both need this
/// exact mapping.
pub(in crate::engines::pending_work) fn resolve_claude_model_for_verify(
    agent: crate::cli::Agent,
    item: &Item,
) -> Option<Result<String, String>> {
    match resolve_claude_model(agent, item) {
        Ok(None) => None,
        Ok(Some(m)) => Some(Ok(m)),
        Err(e) => Some(Err(e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use std::assert_matches;

    use super::*;

    fn nanos() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    }

    fn stage(toml: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("pwf_model_tiers_{}.toml", nanos()));
        std::fs::write(&path, toml).unwrap();
        path
    }

    #[test]
    fn resolves_configured_tier() {
        let path = stage("[tiers.3]\nclaude_model = \"sonnet\"\n");
        let tier = EffortTier::try_from(3u8).unwrap();
        assert_eq!(
            resolve_claude_model_at(path.to_str().unwrap(), tier).unwrap(),
            "sonnet"
        );
    }

    #[test]
    fn missing_file_is_not_found() {
        let missing = std::env::temp_dir().join("pwf_model_tiers_does_not_exist.toml");
        let tier = EffortTier::try_from(1u8).unwrap();
        let err = resolve_claude_model_at(missing.to_str().unwrap(), tier).unwrap_err();
        assert_matches!(err, ModelTiersError::NotFound(_));
    }

    #[test]
    fn malformed_toml_is_parse_error() {
        let path = stage("this is not toml [[[");
        let tier = EffortTier::try_from(1u8).unwrap();
        let err = resolve_claude_model_at(path.to_str().unwrap(), tier).unwrap_err();
        assert_matches!(err, ModelTiersError::Parse(_));
    }

    #[test]
    fn tier_absent_from_table_is_missing_tier() {
        let path = stage("[tiers.1]\nclaude_model = \"sonnet\"\n");
        let tier = EffortTier::try_from(2u8).unwrap();
        let err = resolve_claude_model_at(path.to_str().unwrap(), tier).unwrap_err();
        assert_matches!(err, ModelTiersError::MissingTier { tier: 2, .. });
    }

    #[test]
    fn tier_present_without_claude_model_is_missing_claude_model() {
        let path = stage("[tiers.1]\n");
        let tier = EffortTier::try_from(1u8).unwrap();
        let err = resolve_claude_model_at(path.to_str().unwrap(), tier).unwrap_err();
        assert_matches!(err, ModelTiersError::MissingClaudeModel { tier: 1, .. });
    }

    #[test]
    fn resolve_claude_model_is_none_for_codex_agent() {
        let item = Item {
            effort: Some("3".to_string()),
            ..Item::default_for_test("PWF-0001", "t")
        };
        assert_eq!(
            resolve_claude_model(crate::cli::Agent::Codex, &item).unwrap(),
            None
        );
    }

    #[test]
    fn resolve_claude_model_is_none_when_no_effort_tag() {
        let item = Item::default_for_test("PWF-0001", "t");
        assert_eq!(
            resolve_claude_model(crate::cli::Agent::Claude, &item).unwrap(),
            None
        );
    }

    #[test]
    fn resolve_claude_model_errors_on_corrupted_effort_value() {
        let item = Item {
            effort: Some("nine".to_string()),
            ..Item::default_for_test("PWF-0001", "t")
        };
        let err = resolve_claude_model(crate::cli::Agent::Claude, &item).unwrap_err();
        assert_matches!(
            err,
            crate::engines::pending_work::errors::PendingWorkError::BadEffortValue { ref id, ref value }
                if id == "PWF-0001" && value == "nine"
        );
    }
}
