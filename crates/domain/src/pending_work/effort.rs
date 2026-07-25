use nutype::nutype;

#[nutype(
    validate(greater_or_equal = 1, less_or_equal = 4),
    derive(Debug, Clone, Copy, PartialEq, Eq, TryFrom, Into)
)]
pub struct EffortTier(u8);

#[cfg(test)]
mod tests {
    use super::EffortTier;

    #[test]
    fn try_from_accepts_the_full_valid_range() {
        for n in 1..=4u8 {
            assert_eq!(u8::from(EffortTier::try_from(n).unwrap()), n);
        }
    }
}
