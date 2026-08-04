use std::fmt;

use nutype::nutype;

/// Maximum Unicode scalar count accepted for one pushed prompt.
pub const PUSHED_PROMPT_CHAR_MAX: usize = 2000;

/// Stores bounded, non-empty session prompt text.
#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = PUSHED_PROMPT_CHAR_MAX),
    derive(Debug, Clone, PartialEq, Eq, AsRef, Display, FromStr),
)]
pub struct PushedPrompt(String);

/// Selects a supported agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Agent {
    Claude,
    Codex,
}

/// Selects where an agent runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchMode {
    Inline,
    Multiplexer,
}

/// Selects optional launch-prompt directives.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LaunchDirectives {
    pub worktree: bool,
    pub autonomous: bool,
}

/// Selects the reasoning effort for one agent session.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SessionEffort {
    Low,
    Medium,
    #[default]
    High,
    XHigh,
    Max,
}

impl fmt::Display for SessionEffort {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::XHigh => "xhigh",
            Self::Max => "max",
        })
    }
}

/// Selects an optional agent model override.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AgentModel(Option<String>);

impl From<Option<String>> for AgentModel {
    fn from(value: Option<String>) -> Self {
        Self(value.filter(|model| model != Self::MODEL_DEFAULT))
    }
}

impl From<String> for AgentModel {
    fn from(value: String) -> Self {
        Some(value).into()
    }
}

impl From<Option<&str>> for AgentModel {
    fn from(value: Option<&str>) -> Self {
        value.map(str::to_string).into()
    }
}

impl AgentModel {
    pub const MODEL_DEFAULT: &str = "default";

    pub fn into_inner(self) -> Option<String> {
        self.0
    }

    pub fn display_or_default(&self) -> String {
        self.0.clone().unwrap_or(Self::MODEL_DEFAULT.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::{AgentModel, PUSHED_PROMPT_CHAR_MAX, PushedPrompt};

    #[test]
    fn default_and_absent_models_are_no_override() {
        assert_eq!(AgentModel::from(None::<String>).into_inner(), None);
        assert_eq!(
            AgentModel::from(Some("default".to_string())).into_inner(),
            None
        );
        assert_eq!(
            AgentModel::from(Some("gpt-5.6".to_string())).into_inner(),
            Some("gpt-5.6".to_string())
        );
    }

    #[test]
    fn pushed_prompt_trims_outer_whitespace_and_preserves_internal_formatting() {
        let prompt = PushedPrompt::try_new("  first line\n  indented line  ").unwrap();

        assert_eq!(prompt.as_ref(), "first line\n  indented line");
    }

    #[test]
    fn pushed_prompt_rejects_blank_and_oversized_values() {
        assert!(PushedPrompt::try_new(" \n\t ").is_err());
        assert!(PushedPrompt::try_new("é".repeat(PUSHED_PROMPT_CHAR_MAX)).is_ok());
        assert!(PushedPrompt::try_new("é".repeat(PUSHED_PROMPT_CHAR_MAX + 1)).is_err());
    }
}
