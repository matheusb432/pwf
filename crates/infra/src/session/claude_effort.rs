use pwf_models::session::SessionEffort;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ClaudeEffort {
    Low,
    Medium,
    High,
    XHigh,
    Max,
}

impl From<SessionEffort> for ClaudeEffort {
    fn from(effort: SessionEffort) -> Self {
        match effort {
            SessionEffort::Low => Self::Low,
            SessionEffort::Medium => Self::Medium,
            SessionEffort::High => Self::High,
            SessionEffort::XHigh => Self::XHigh,
            SessionEffort::Max => Self::Max,
        }
    }
}

impl ClaudeEffort {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::XHigh => "xhigh",
            Self::Max => "max",
        }
    }
}
