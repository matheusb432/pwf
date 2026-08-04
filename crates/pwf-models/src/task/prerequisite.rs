use std::{fmt, str::FromStr};

use thiserror::Error;

use super::TaskId;

/// Contains the task IDs parsed from one prerequisite argument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrerequisiteInput(Prerequisites);

impl PrerequisiteInput {
    /// Returns the parsed task IDs in encounter order.
    pub fn iter(&self) -> impl Iterator<Item = &TaskId> {
        self.0.iter()
    }
}

impl FromStr for PrerequisiteInput {
    type Err = PrerequisiteInputError;

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
                    .map_err(|_| PrerequisiteInputError::InvalidId {
                        raw: raw.to_string(),
                    })?;
            identifiers.push(identifier);
        }
        Prerequisites::try_new(identifiers)
            .map(Self)
            .map_err(|_| PrerequisiteInputError::MissingId)
    }
}

/// Stores a non-empty, ordered set of prerequisite task IDs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prerequisites(Vec<TaskId>);

impl Prerequisites {
    /// Combines parsed prerequisite arguments, returning `None` when no arguments were supplied.
    #[must_use]
    pub fn from_inputs(inputs: &[PrerequisiteInput]) -> Option<Self> {
        let mut identifiers = inputs.iter().flat_map(PrerequisiteInput::iter).cloned();
        let first = identifiers.next()?;
        Some(Self::from_first_and_rest(first, identifiers))
    }

    /// Creates a deduplicated prerequisite set in first-seen order.
    ///
    /// # Errors
    ///
    /// Returns [`EmptyPrerequisitesError`] when no IDs are supplied.
    pub fn try_new(identifiers: Vec<TaskId>) -> Result<Self, EmptyPrerequisitesError> {
        let mut identifiers = identifiers.into_iter();
        let first = identifiers.next().ok_or(EmptyPrerequisitesError)?;
        Ok(Self::from_first_and_rest(first, identifiers))
    }

    /// Returns prerequisite IDs in encounter order.
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

impl fmt::Display for Prerequisites {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = self
            .iter()
            .map(|identifier| format!("[[{identifier}]]"))
            .collect::<Vec<_>>()
            .join(", ");
        formatter.write_str(&value)
    }
}

/// Reports invalid prerequisite argument syntax.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PrerequisiteInputError {
    /// A non-empty segment is not a task ID.
    #[error("invalid prerequisite task ID: {raw}")]
    InvalidId { raw: String },
    /// The argument contains no task IDs.
    #[error("prerequisite input requires a task ID")]
    MissingId,
}

/// Reports an empty prerequisite collection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("prerequisites cannot be empty")]
pub struct EmptyPrerequisitesError;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_parses_compact_comma_separated_and_wikilink_ids() {
        let input = "cfg57, [[CFG-0014]], CFG-14"
            .parse::<PrerequisiteInput>()
            .unwrap();

        assert_eq!(
            input.iter().map(AsRef::as_ref).collect::<Vec<_>>(),
            ["CFG-0057", "CFG-0014"]
        );
        assert_eq!(input.0.to_string(), "[[CFG-0057]], [[CFG-0014]]");
    }

    #[test]
    fn input_rejects_empty_and_invalid_values() {
        assert!(matches!(
            " , ".parse::<PrerequisiteInput>(),
            Err(PrerequisiteInputError::MissingId)
        ));
        assert!(matches!(
            "PWF-99999".parse::<PrerequisiteInput>(),
            Err(PrerequisiteInputError::InvalidId { raw }) if raw == "PWF-99999"
        ));
    }

    #[test]
    fn merge_preserves_first_seen_order() {
        let existing = "pwf1, cfg14".parse::<PrerequisiteInput>().unwrap().0;
        let appended = "cfg14, alt2".parse::<PrerequisiteInput>().unwrap().0;

        assert_eq!(
            existing
                .merge(&appended)
                .iter()
                .map(AsRef::as_ref)
                .collect::<Vec<_>>(),
            ["PWF-0001", "CFG-0014", "ALT-0002"]
        );
    }
}
