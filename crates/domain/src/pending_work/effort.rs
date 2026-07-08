use nutype::nutype;

#[nutype(
    validate(greater_or_equal = 1, less_or_equal = 4),
    derive(Debug, Clone, Copy, PartialEq, Eq, TryFrom, Into)
)]
pub struct EffortTier(u8);

impl EffortTier {
    pub fn parse(raw: &str) -> Option<Self> {
        raw.trim()
            .parse::<u8>()
            .ok()
            .and_then(|value| Self::try_from(value).ok())
    }
}

#[cfg(test)]
mod tests {
    use super::EffortTier;

    #[test]
    fn try_from_accepts_the_full_valid_range() {
        for n in 1..=4u8 {
            assert_eq!(u8::from(EffortTier::try_from(n).unwrap()), n);
        }
    }

    #[test]
    fn parse_rejects_out_of_range_and_non_numeric() {
        assert!(EffortTier::parse("0").is_none());
        assert!(EffortTier::parse("5").is_none());
        assert!(EffortTier::parse("abc").is_none());
        assert!(EffortTier::parse("").is_none());
    }
}
