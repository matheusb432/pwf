use pwf_application::pending_work::session::SessionEffort;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CodexReasoningEffort {
    Low,
    Medium,
    High,
    XHigh,
}

impl From<SessionEffort> for CodexReasoningEffort {
    fn from(effort: SessionEffort) -> Self {
        match effort {
            SessionEffort::Low => Self::Low,
            SessionEffort::Medium => Self::Medium,
            SessionEffort::High => Self::High,
            SessionEffort::XHigh => Self::XHigh,
        }
    }
}

impl CodexReasoningEffort {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::XHigh => "xhigh",
        }
    }
}
