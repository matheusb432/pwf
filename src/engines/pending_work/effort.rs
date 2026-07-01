//! An item's optional effort/complexity tier (1=easy .. 4=xhard). Read from the
//! `effort:` frontmatter key; resolved to a Claude model via
//! `config/model-tiers.toml` at `pwf session`/`pwf verify` time.

use nutype::nutype;

/// A validated 1-4 effort/complexity tier. Invalid states (0, 5+, non-numeric)
/// are unrepresentable past construction.
#[nutype(
    validate(greater_or_equal = 1, less_or_equal = 4),
    derive(Debug, Clone, Copy, PartialEq, Eq, TryFrom, Into)
)]
pub struct EffortTier(u8);

impl EffortTier {
    /// Parse a raw `effort:` frontmatter value (e.g. `"3"`). `None` on anything
    /// that isn't an in-range integer 1-4 — including a hand-corrupted note.
    pub fn parse(raw: &str) -> Option<Self> {
        raw.trim()
            .parse::<u8>()
            .ok()
            .and_then(|n| Self::try_from(n).ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn try_from_accepts_the_full_valid_range() {
        for n in 1..=4u8 {
            assert_eq!(u8::from(EffortTier::try_from(n).unwrap()), n);
        }
    }

    #[test]
    fn try_from_rejects_zero_and_five() {
        assert!(EffortTier::try_from(0u8).is_err());
        assert!(EffortTier::try_from(5u8).is_err());
    }

    #[test]
    fn parse_accepts_trimmed_digits() {
        assert_eq!(u8::from(EffortTier::parse(" 3 ").unwrap()), 3);
    }

    #[test]
    fn parse_rejects_out_of_range_and_non_numeric() {
        assert!(EffortTier::parse("0").is_none());
        assert!(EffortTier::parse("5").is_none());
        assert!(EffortTier::parse("abc").is_none());
        assert!(EffortTier::parse("").is_none());
    }
}
