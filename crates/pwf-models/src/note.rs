//! Defines project-note identifiers.

use std::fmt;

use crate::project::ProjectId;

/// Describes one project note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectNote {
    /// Identifies the note within its project.
    pub id: NoteId,
    /// Names the note.
    pub title: String,
}

/// Stores a `{PROJECT_ID}-NOTE-NNNN` identifier.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NoteId {
    value: String,
    project_id: ProjectId,
    number: u32,
}

impl NoteId {
    /// Creates an identifier from its spelling.
    ///
    /// # Errors
    ///
    /// Returns [`NoteIdError`] unless the value contains a valid project ID, `-NOTE-`, and
    /// exactly four decimal digits.
    pub fn try_new(raw: impl Into<String>) -> Result<Self, NoteIdError> {
        let value = raw.into();
        let Some((raw_project_id, number)) = value.split_once("-NOTE-") else {
            return Err(NoteIdError { value });
        };
        let Ok(project_id) = ProjectId::try_new(raw_project_id) else {
            return Err(NoteIdError { value });
        };
        let valid_project_id = project_id.as_ref() == raw_project_id;
        let valid_number =
            number.len() == 4 && number.chars().all(|character| character.is_ascii_digit());
        if !valid_project_id || !valid_number {
            return Err(NoteIdError { value });
        }
        let number = number.parse().map_err(|_| NoteIdError {
            value: value.clone(),
        })?;
        Ok(Self {
            value,
            project_id,
            number,
        })
    }

    /// Returns the project ID encoded in this note ID.
    #[must_use]
    pub fn project_id(&self) -> &ProjectId {
        &self.project_id
    }

    /// Returns the decimal numeric suffix.
    #[must_use]
    pub fn number(&self) -> u32 {
        self.number
    }
}

impl AsRef<str> for NoteId {
    fn as_ref(&self) -> &str {
        &self.value
    }
}

impl fmt::Display for NoteId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.value)
    }
}

/// Reports an invalid project-note identifier.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid note id {value:?}")]
pub struct NoteIdError {
    value: String,
}

#[cfg(test)]
mod tests {
    use super::{NoteId, ProjectId};

    #[test]
    fn valid_full_identifiers_expose_number() {
        for raw in ["PW-NOTE-0042", "PWF-NOTE-0042", "TOOL-NOTE-0042"] {
            let id = NoteId::try_new(raw).unwrap();
            let project_id = raw.split_once("-NOTE-").unwrap().0;

            assert_eq!(id.as_ref(), raw);
            assert_eq!(id.project_id(), &ProjectId::try_new(project_id).unwrap());
            assert_eq!(id.number(), 42);
        }
    }

    #[test]
    fn identifier_rejects_invalid_project_ids_and_suffix_shapes() {
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
