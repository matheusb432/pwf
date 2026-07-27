use std::fmt;

/// Selects the reasoning effort for one agent session.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SessionEffort {
    Low,
    Medium,
    #[default]
    High,
    XHigh,
}

impl fmt::Display for SessionEffort {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::XHigh => "xhigh",
        })
    }
}
