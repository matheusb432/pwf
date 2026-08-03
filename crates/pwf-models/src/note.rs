//! Defines canonical project-note identifiers.

use std::fmt;

/// Describes one project note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectNote {
    /// Identifies the note within its project.
    pub id: NoteId,
    /// Names the note.
    pub title: String,
}

/// Stores a canonical `{PREFIX}-NOTE-NNNN` identifier.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NoteId {
    canonical: String,
    number: u32,
}

impl NoteId {
    /// Creates an identifier from its canonical spelling.
    ///
    /// # Errors
    ///
    /// Returns [`NoteIdError`] unless the value contains a two-to-four-letter uppercase prefix,
    /// `-NOTE-`, and exactly four decimal digits.
    pub fn try_new(raw: impl Into<String>) -> Result<Self, NoteIdError> {
        let canonical = raw.into();
        let Some((prefix, number)) = canonical.split_once("-NOTE-") else {
            return Err(NoteIdError { value: canonical });
        };
        let valid_prefix = (2..=4).contains(&prefix.len())
            && prefix
                .chars()
                .all(|character| character.is_ascii_uppercase());
        let valid_number =
            number.len() == 4 && number.chars().all(|character| character.is_ascii_digit());
        if !valid_prefix || !valid_number {
            return Err(NoteIdError { value: canonical });
        }
        let number = number.parse().map_err(|_| NoteIdError {
            value: canonical.clone(),
        })?;
        Ok(Self { canonical, number })
    }

    /// Returns the decimal numeric suffix.
    #[must_use]
    pub fn number(&self) -> u32 {
        self.number
    }
}

impl AsRef<str> for NoteId {
    fn as_ref(&self) -> &str {
        &self.canonical
    }
}

impl fmt::Display for NoteId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.canonical)
    }
}

/// Reports a non-canonical project-note identifier.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid canonical note id {value:?}")]
pub struct NoteIdError {
    value: String,
}

#[cfg(test)]
mod tests {
    use super::NoteId;

    #[test]
    fn canonical_full_identifier_exposes_number() {
        let id = NoteId::try_new("PWF-NOTE-0042").unwrap();

        assert_eq!(id.as_ref(), "PWF-NOTE-0042");
        assert_eq!(id.number(), 42);
    }

    #[test]
    fn canonical_identifier_rejects_invalid_prefix_and_suffix_shapes() {
        for raw in [
            "pwf-NOTE-0001",
            "P-NOTE-0001",
            "TOOLS-NOTE-0001",
            "PWF-NOTE-001",
            "PWF-NOTE-00001",
            "PWF-NOTE-abcd",
            "PWF-0001",
        ] {
            assert!(NoteId::try_new(raw).is_err(), "accepted {raw}");
        }
    }
}
