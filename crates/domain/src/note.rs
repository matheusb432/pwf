//! Defines canonical project-note identifiers.

use std::fmt;

/// Stores a canonical `{PREFIX}-NOTE-NNNN` identifier.
///
/// # Examples
///
/// ```
/// use pwf_domain::note::NoteId;
///
/// let id = NoteId::try_new("PWF-NOTE-0042").unwrap();
/// assert_eq!(id.as_ref(), "PWF-NOTE-0042");
/// ```
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
    ///
    /// # Examples
    ///
    /// ```
    /// use pwf_domain::note::NoteId;
    ///
    /// assert!(NoteId::try_new("PWF-NOTE-0001").is_ok());
    /// assert!(NoteId::try_new(concat!("pwf", "-note-0001")).is_err());
    /// ```
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
    ///
    /// # Examples
    ///
    /// ```
    /// use pwf_domain::note::NoteId;
    ///
    /// assert_eq!(NoteId::try_new("PWF-NOTE-0042").unwrap().number(), 42);
    /// ```
    #[must_use]
    pub fn number(&self) -> u32 {
        self.number
    }

    /// Returns the canonical Markdown file name.
    ///
    /// # Examples
    ///
    /// ```
    /// use pwf_domain::note::NoteId;
    ///
    /// let id = NoteId::try_new("PWF-NOTE-0042").unwrap();
    /// assert_eq!(id.file_name(), "PWF-NOTE-0042.md");
    /// ```
    #[must_use]
    pub fn file_name(&self) -> String {
        format!("{}.md", self.canonical)
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
///
/// # Examples
///
/// ```
/// use pwf_domain::note::NoteId;
///
/// let error = NoteId::try_new("PWF-0001").unwrap_err();
/// assert_eq!(error.to_string(), "invalid canonical note id \"PWF-0001\"");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid canonical note id {value:?}")]
pub struct NoteIdError {
    value: String,
}

#[cfg(test)]
mod tests {
    use super::NoteId;

    #[test]
    fn canonical_full_identifier_exposes_number_and_file_name() {
        let id = NoteId::try_new("PWF-NOTE-0042").unwrap();

        assert_eq!(id.as_ref(), "PWF-NOTE-0042");
        assert_eq!(id.number(), 42);
        assert_eq!(id.file_name(), "PWF-NOTE-0042.md");
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
