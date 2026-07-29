//! Resolves Claude model selection from explicit overrides and effort-tier entries.

use std::error::Error;

use pwf_models::pending_work::EffortTier;
use thiserror::Error;

use super::{Agent, ModelTierCatalog, ModelTierLookup};

/// Reports invalid effort values and incomplete model-tier configuration.
#[derive(Debug, Error)]
pub(super) enum ModelSelectionError {
    #[error(
        "item {task_id} has an invalid effort value '{value}' (expected low, medium, high, or highest)."
    )]
    InvalidEffort { task_id: String, value: String },
    #[error("{0}")]
    Catalog(#[source] Box<dyn Error + Send + Sync>),
    #[error("tier {tier} has no [tiers.{tier}] entry in {catalog}")]
    MissingTier { tier: EffortTier, catalog: String },
    #[error("tier {tier} in {catalog} has no claude_model set")]
    MissingClaudeModel { tier: EffortTier, catalog: String },
}

pub(super) fn resolve_model<C>(
    catalog: &C,
    agent: Agent,
    task_id: &str,
    effort: Option<&str>,
) -> Result<Option<String>, ModelSelectionError>
where
    C: ModelTierCatalog,
{
    if agent == Agent::Codex {
        return Ok(None);
    }
    let Some(raw_effort) = effort else {
        return Ok(None);
    };
    let tier = parse_effort(raw_effort).ok_or_else(|| ModelSelectionError::InvalidEffort {
        task_id: task_id.to_string(),
        value: raw_effort.to_string(),
    })?;
    let ModelTierLookup {
        catalog,
        tier: entry,
    } = catalog
        .tier(tier)
        .map_err(|error| ModelSelectionError::Catalog(Box::new(error)))?;
    let Some(entry) = entry else {
        return Err(ModelSelectionError::MissingTier { tier, catalog });
    };
    let model = entry
        .claude_model
        .ok_or(ModelSelectionError::MissingClaudeModel { tier, catalog })?;
    Ok(if model.is_empty() { None } else { Some(model) })
}

fn parse_effort(raw: &str) -> Option<EffortTier> {
    raw.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use std::{assert_matches, error::Error, fmt};

    use pwf_models::pending_work::EffortTier;

    use super::{ModelSelectionError, parse_effort, resolve_model};
    use crate::pending_work::session::{Agent, ModelTier, ModelTierCatalog, ModelTierLookup};

    const CATALOG_PATH: &str = "/config/model-tiers.toml";

    #[derive(Debug, Clone)]
    struct CatalogError(&'static str);

    impl fmt::Display for CatalogError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str(self.0)
        }
    }

    impl Error for CatalogError {}

    #[derive(Clone)]
    struct Catalog {
        result: Result<ModelTierLookup, CatalogError>,
    }

    impl ModelTierCatalog for Catalog {
        type Error = CatalogError;

        fn tier(&self, _effort: EffortTier) -> Result<ModelTierLookup, Self::Error> {
            self.result.clone()
        }
    }

    fn catalog(claude_model: Option<&str>) -> Catalog {
        Catalog {
            result: Ok(ModelTierLookup {
                catalog: CATALOG_PATH.to_string(),
                tier: Some(ModelTier {
                    claude_model: claude_model.map(str::to_string),
                }),
            }),
        }
    }

    #[test]
    fn effort_text_decodes_only_plain_english_names() {
        for (raw, expected) in [
            ("low", EffortTier::Low),
            ("medium", EffortTier::Medium),
            ("high", EffortTier::High),
            ("highest", EffortTier::Highest),
        ] {
            assert_eq!(parse_effort(raw), Some(expected));
        }
        for raw in ["1", "4", "abc", ""] {
            assert!(parse_effort(raw).is_none());
        }
    }

    #[test]
    fn codex_ignores_effort_and_the_catalog() {
        let unavailable_catalog = Catalog {
            result: Err(CatalogError("catalog unavailable")),
        };

        let model =
            resolve_model(&unavailable_catalog, Agent::Codex, "PWF-0001", Some("nine")).unwrap();

        assert_eq!(model, None);
    }

    #[test]
    fn claude_without_effort_does_not_read_the_catalog() {
        let unavailable_catalog = Catalog {
            result: Err(CatalogError("catalog unavailable")),
        };

        let model = resolve_model(&unavailable_catalog, Agent::Claude, "PWF-0001", None).unwrap();

        assert_eq!(model, None);
    }

    #[test]
    fn malformed_effort_is_an_application_error() {
        let error = resolve_model(
            &catalog(Some("sonnet")),
            Agent::Claude,
            "PWF-0001",
            Some("nine"),
        )
        .unwrap_err();

        assert_matches!(
            error,
            ModelSelectionError::InvalidEffort { ref task_id, ref value }
                if task_id == "PWF-0001" && value == "nine"
        );
    }

    #[test]
    fn configured_claude_model_is_selected() {
        let model = resolve_model(
            &catalog(Some("sonnet")),
            Agent::Claude,
            "PWF-0001",
            Some("high"),
        )
        .unwrap();

        assert_eq!(model.as_deref(), Some("sonnet"));
    }

    #[test]
    fn empty_claude_model_is_the_no_override_sentinel() {
        let model = resolve_model(
            &catalog(Some("")),
            Agent::Claude,
            "PWF-0001",
            Some("medium"),
        )
        .unwrap();

        assert_eq!(model, None);
    }

    #[test]
    fn catalog_read_error_retains_its_source() {
        let unavailable_catalog = Catalog {
            result: Err(CatalogError("catalog unavailable")),
        };

        let error = resolve_model(&unavailable_catalog, Agent::Claude, "PWF-0001", Some("low"))
            .unwrap_err();

        assert_eq!(error.source().unwrap().to_string(), "catalog unavailable");
    }

    #[test]
    fn missing_tier_is_an_application_error() {
        let missing = Catalog {
            result: Ok(ModelTierLookup {
                catalog: CATALOG_PATH.to_string(),
                tier: None,
            }),
        };

        let error =
            resolve_model(&missing, Agent::Claude, "PWF-0001", Some("highest")).unwrap_err();

        assert_matches!(
            &error,
            ModelSelectionError::MissingTier {
                tier: EffortTier::Highest,
                ..
            }
        );
        assert_eq!(
            error.to_string(),
            format!("tier highest has no [tiers.highest] entry in {CATALOG_PATH}")
        );
    }

    #[test]
    fn missing_claude_model_is_an_application_error() {
        let error =
            resolve_model(&catalog(None), Agent::Claude, "PWF-0001", Some("highest")).unwrap_err();

        assert_matches!(
            &error,
            ModelSelectionError::MissingClaudeModel {
                tier: EffortTier::Highest,
                ..
            }
        );
        assert_eq!(
            error.to_string(),
            format!("tier highest in {CATALOG_PATH} has no claude_model set")
        );
    }
}
