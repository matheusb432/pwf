/// Thin date carrier (YYYY-MM-DD as authored). Deliberately non-validating:
/// a validating newtype would change behavior under the byte-identical gate.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Timestamp(String);

impl Timestamp {
    /// Creates a timestamp from raw string input.
    pub fn new(raw: impl Into<String>) -> Self {
        Self(raw.into())
    }

    /// Returns the timestamp string as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_roundtrips_raw_string() {
        let raw = "2026-07-15";
        let ts = Timestamp::new(raw);
        assert_eq!(ts.as_str(), raw);
    }
}
