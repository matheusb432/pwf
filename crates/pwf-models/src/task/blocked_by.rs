use std::{fmt, str::FromStr};

use thiserror::Error;

use super::TaskId;

/// Contains the task IDs parsed from one blocked-by argument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockedByInput(BlockedBy);

impl BlockedByInput {
    /// Returns the parsed task IDs in encounter order.
    fn iter(&self) -> impl Iterator<Item = &TaskId> {
        self.0.iter()
    }
}

impl FromStr for BlockedByInput {
    type Err = BlockedByInputError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let mut identifiers = Vec::new();
        for raw in value
            .split(',')
            .map(str::trim)
            .filter(|raw| !raw.is_empty())
        {
            let candidate = raw
                .strip_prefix("[[")
                .and_then(|trimmed| trimmed.strip_suffix("]]"))
                .unwrap_or(raw);
            let identifier =
                candidate
                    .parse::<TaskId>()
                    .map_err(|_| BlockedByInputError::InvalidId {
                        raw: raw.to_string(),
                    })?;
            identifiers.push(identifier);
        }
        BlockedBy::try_new(identifiers)
            .map(Self)
            .map_err(|_| BlockedByInputError::MissingId)
    }
}

/// Stores a non-empty, ordered blocked-by relationship.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockedBy(Vec<TaskId>);

impl BlockedBy {
    /// Combines parsed blocked-by arguments, returning `None` when no arguments were supplied.
    #[must_use]
    pub fn from_inputs(inputs: &[BlockedByInput]) -> Option<Self> {
        let mut identifiers = inputs.iter().flat_map(BlockedByInput::iter).cloned();
        let first = identifiers.next()?;
        Some(Self::from_first_and_rest(first, identifiers))
    }

    /// Creates a deduplicated blocked-by relationship in first-seen order.
    ///
    /// # Errors
    ///
    /// Returns [`EmptyBlockedByError`] when no IDs are supplied.
    pub fn try_new(identifiers: Vec<TaskId>) -> Result<Self, EmptyBlockedByError> {
        let mut identifiers = identifiers.into_iter();
        let first = identifiers.next().ok_or(EmptyBlockedByError)?;
        Ok(Self::from_first_and_rest(first, identifiers))
    }

    /// Returns blocked-by task IDs in encounter order.
    pub fn iter(&self) -> impl Iterator<Item = &TaskId> {
        self.0.iter()
    }

    /// Appends unseen task IDs while preserving first-seen order.
    #[must_use]
    pub fn merge(&self, appended: &Self) -> Self {
        let mut merged = self.0.clone();
        for identifier in appended.iter() {
            if !merged.contains(identifier) {
                merged.push(identifier.clone());
            }
        }
        Self(merged)
    }

    fn from_first_and_rest(first: TaskId, identifiers: impl IntoIterator<Item = TaskId>) -> Self {
        let mut unique = vec![first];
        for identifier in identifiers {
            if !unique.contains(&identifier) {
                unique.push(identifier);
            }
        }
        Self(unique)
    }
}

impl fmt::Display for BlockedBy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = self
            .iter()
            .map(|identifier| format!("[[{identifier}]]"))
            .collect::<Vec<_>>()
            .join(", ");
        formatter.write_str(&value)
    }
}

/// Reports invalid blocked-by argument syntax.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BlockedByInputError {
    /// A non-empty segment is not a task ID.
    #[error("invalid blocked-by task ID: {raw}")]
    InvalidId { raw: String },
    /// The argument contains no task IDs.
    #[error("blocked-by input requires a task ID")]
    MissingId,
}

/// Reports an empty blocked-by relationship.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("blocked_by cannot be empty")]
pub struct EmptyBlockedByError;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_parses_compact_comma_separated_and_wikilink_ids() {
        let input = "aux57, [[AUX-0014]], AUX-14"
            .parse::<BlockedByInput>()
            .unwrap();

        assert_eq!(
            input.iter().map(AsRef::as_ref).collect::<Vec<_>>(),
            ["AUX-0057", "AUX-0014"]
        );
        assert_eq!(input.0.to_string(), "[[AUX-0057]], [[AUX-0014]]");
    }

    #[test]
    fn input_rejects_empty_and_invalid_values() {
        assert!(matches!(
            " , ".parse::<BlockedByInput>(),
            Err(BlockedByInputError::MissingId)
        ));
        assert!(matches!(
            "PWF-99999".parse::<BlockedByInput>(),
            Err(BlockedByInputError::InvalidId { raw }) if raw == "PWF-99999"
        ));
    }

    #[test]
    fn merge_preserves_first_seen_order() {
        let existing = "pwf1, aux14".parse::<BlockedByInput>().unwrap().0;
        let appended = "aux14, alt2".parse::<BlockedByInput>().unwrap().0;

        assert_eq!(
            existing
                .merge(&appended)
                .iter()
                .map(AsRef::as_ref)
                .collect::<Vec<_>>(),
            ["PWF-0001", "AUX-0014", "ALT-0002"]
        );
    }
}
