/// Preserves an authored `YYYY-MM-DD` value without validating it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Timestamp(String);

impl Timestamp {
    pub fn new(raw: impl Into<String>) -> Self {
        Self(raw.into())
    }

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
