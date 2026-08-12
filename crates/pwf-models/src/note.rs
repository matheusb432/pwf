//! Defines project-note values and identifiers.

use std::{fmt, str::FromStr};

use nutype::nutype;

use crate::project::ProjectId;

/// Maximum Unicode scalar count accepted for one project-note title.
pub const NOTE_TITLE_CHARACTER_LIMIT: usize = 200;

/// Stores a bounded, non-empty project-note title.
#[nutype(
    sanitize(with = normalize_inline),
    validate(not_empty, len_char_max = NOTE_TITLE_CHARACTER_LIMIT),
    derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, AsRef, Display, FromStr),
)]
pub struct NoteTitle(String);

/// Stores non-empty Markdown content for one project note.
#[nutype(
    sanitize(trim),
    validate(not_empty),
    derive(Debug, Clone, PartialEq, Eq, AsRef, Display, FromStr)
)]
pub struct NoteContent(String);

/// Stores a non-empty explanation of why a project note matters.
#[nutype(
    sanitize(trim),
    validate(not_empty),
    derive(Debug, Clone, PartialEq, Eq, AsRef, Display, FromStr)
)]
pub struct NoteWhy(String);

/// Stores a non-empty project-note subject classification.
#[nutype(
    sanitize(with = normalize_inline),
    validate(not_empty),
    derive(Debug, Clone, PartialEq, Eq, AsRef, Display, FromStr),
)]
pub struct NoteDomain(String);

/// Stores one non-empty project-note discovery label.
#[nutype(
    sanitize(with = normalize_inline),
    validate(not_empty),
    derive(Debug, Clone, PartialEq, Eq, AsRef, Display, FromStr),
)]
pub struct NoteTag(String);

/// Stores one non-empty project-note evidence source.
#[nutype(
    sanitize(with = normalize_inline),
    validate(not_empty),
    derive(Debug, Clone, PartialEq, Eq, AsRef, Display, FromStr),
)]
pub struct NoteSource(String);

/// Stores one non-empty project-note verification marker.
#[nutype(
    sanitize(with = normalize_inline),
    validate(not_empty),
    derive(Debug, Clone, PartialEq, Eq, AsRef, Display, FromStr),
)]
pub struct NoteVerification(String);

/// Describes one project note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectNote {
    /// Identifies the note within its project.
    pub id: NoteId,
    /// Names the note.
    pub title: NoteTitle,
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

/// Selects a project note by full ID, `NOTE-NNNN`, or bare numeric suffix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteSelector {
    project_id: Option<ProjectId>,
    number: u32,
}

impl NoteSelector {
    /// Resolves the selector for `project_id`, returning `None` for a different explicit project.
    #[must_use]
    pub fn resolve(&self, project_id: &ProjectId) -> Option<NoteId> {
        if self
            .project_id
            .as_ref()
            .is_some_and(|selected| selected != project_id)
        {
            return None;
        }
        Some(NoteId {
            value: format!("{project_id}-NOTE-{:04}", self.number),
            project_id: project_id.clone(),
            number: self.number,
        })
    }
}

impl FromStr for NoteSelector {
    type Err = NoteSelectorError;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        let normalized = raw.trim().to_ascii_uppercase();
        let invalid = || NoteSelectorError {
            raw: raw.to_string(),
        };
        let (project_id, digits, exact_width) =
            if let Some((project, digits)) = normalized.split_once("-NOTE-") {
                let project_id = ProjectId::try_new(project).map_err(|_| invalid())?;
                (Some(project_id), digits, true)
            } else if let Some(digits) = normalized.strip_prefix("NOTE-") {
                (None, digits, true)
            } else {
                (None, normalized.as_str(), false)
            };
        let valid_width = if exact_width {
            digits.len() == 4
        } else {
            (1..=4).contains(&digits.len())
        };
        if !valid_width || !digits.chars().all(|character| character.is_ascii_digit()) {
            return Err(invalid());
        }
        let number = digits.parse::<u32>().map_err(|_| invalid())?;
        if number > 9_999 {
            return Err(invalid());
        }
        Ok(Self { project_id, number })
    }
}

impl fmt::Display for NoteSelector {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.project_id {
            Some(project_id) => write!(formatter, "{project_id}-NOTE-{:04}", self.number),
            None => write!(formatter, "NOTE-{:04}", self.number),
        }
    }
}

/// Reports invalid project-note selector syntax.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid note selector {raw:?}; expected PROJECT-NOTE-NNNN, NOTE-NNNN, or a number")]
pub struct NoteSelectorError {
    raw: String,
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

#[expect(
    clippy::needless_pass_by_value,
    reason = "nutype string sanitizers receive owned values"
)]
fn normalize_inline(value: String) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
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
