use thiserror::Error;

use super::TaskId;

/// Stores a non-empty, ordered blocked-by relationship.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockedBy(Vec<TaskId>);

impl BlockedBy {
    /// Creates a deduplicated blocked-by relationship in first-seen order.
    ///
    /// # Errors
    ///
    /// Returns [`EmptyBlockedByError`] when no IDs are supplied.
    pub fn try_new(
        identifiers: impl IntoIterator<Item = TaskId>,
    ) -> Result<Self, EmptyBlockedByError> {
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
            push_unseen_identifier(&mut merged, identifier.clone());
        }
        Self(merged)
    }

    fn from_first_and_rest(first: TaskId, identifiers: impl IntoIterator<Item = TaskId>) -> Self {
        let mut unique = vec![first];
        for identifier in identifiers {
            push_unseen_identifier(&mut unique, identifier);
        }
        Self(unique)
    }
}

fn push_unseen_identifier(identifiers: &mut Vec<TaskId>, identifier: TaskId) {
    if !identifiers.contains(&identifier) {
        identifiers.push(identifier);
    }
}

/// Reports an empty blocked-by relationship.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("blocked_by cannot be empty")]
pub struct EmptyBlockedByError;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_preserves_first_seen_order() {
        let existing =
            BlockedBy::try_new(["foo1", "aux14"].into_iter().map(|id| id.parse().unwrap()))
                .unwrap();
        let appended =
            BlockedBy::try_new(["aux14", "alt2"].into_iter().map(|id| id.parse().unwrap()))
                .unwrap();

        assert_eq!(
            existing
                .merge(&appended)
                .iter()
                .map(AsRef::as_ref)
                .collect::<Vec<_>>(),
            ["FOO-0001", "AUX-0014", "ALT-0002"]
        );
    }
}
