use std::{fmt, str::FromStr};

use thiserror::Error;

/// Classifies a task's scheduling priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PriorityTier {
    Low,
    Medium,
    High,
    Highest,
}

impl AsRef<str> for PriorityTier {
    fn as_ref(&self) -> &str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Highest => "highest",
        }
    }
}

impl fmt::Display for PriorityTier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_ref())
    }
}

impl FromStr for PriorityTier {
    type Err = PriorityTierError;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        match raw {
            "low" => Ok(Self::Low),
            "medium" => Ok(Self::Medium),
            "high" => Ok(Self::High),
            "highest" => Ok(Self::Highest),
            _ => Err(PriorityTierError),
        }
    }
}

/// Reports an unsupported task priority name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("expected one of: low, medium, high, highest")]
pub struct PriorityTierError;
