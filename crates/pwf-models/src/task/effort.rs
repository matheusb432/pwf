use std::{fmt, str::FromStr};

/// Classifies the effort required to complete a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffortTier {
    Low,
    Medium,
    High,
    Highest,
}

impl AsRef<str> for EffortTier {
    fn as_ref(&self) -> &str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Highest => "highest",
        }
    }
}

impl fmt::Display for EffortTier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_ref())
    }
}

impl FromStr for EffortTier {
    type Err = &'static str;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        match raw {
            "low" => Ok(Self::Low),
            "medium" => Ok(Self::Medium),
            "high" => Ok(Self::High),
            "highest" => Ok(Self::Highest),
            _ => Err("expected one of: low, medium, high, highest"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::EffortTier;

    #[test]
    fn plain_english_names_round_trip() {
        for (raw, expected) in [
            ("low", EffortTier::Low),
            ("medium", EffortTier::Medium),
            ("high", EffortTier::High),
            ("highest", EffortTier::Highest),
        ] {
            let effort = raw.parse::<EffortTier>().unwrap();

            assert_eq!(effort, expected);
            assert_eq!(effort.to_string(), raw);
        }
    }

    #[test]
    fn numeric_tiers_are_not_accepted_as_effort_names() {
        for raw in ["1", "2", "3", "4"] {
            assert!(raw.parse::<EffortTier>().is_err(), "{raw} was accepted");
        }
    }
}
