use std::str::FromStr;

use thiserror::Error;

use crate::project::ProjectId;

/// Identifies one task and retains its validated project identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TaskId {
    value: String,
    project_id: ProjectId,
    number: u16,
}

impl TaskId {
    /// Creates a task ID from its full stored spelling.
    ///
    /// # Errors
    ///
    /// Returns [`TaskIdError`] unless the value contains a valid project ID,
    /// a separator, and exactly four decimal digits.
    pub fn try_new(raw: impl Into<String>) -> Result<Self, TaskIdError> {
        let value = raw.into();
        let Some((code, digits)) = value.split_once('-') else {
            return Err(TaskIdError { value });
        };
        let Ok(project_id) = ProjectId::try_new(code) else {
            return Err(TaskIdError { value });
        };
        let valid_project_id = project_id.as_ref() == code;
        let valid_number =
            digits.len() == 4 && digits.chars().all(|character| character.is_ascii_digit());
        if !valid_project_id || !valid_number {
            return Err(TaskIdError { value });
        }
        let number = digits.parse().map_err(|_| TaskIdError {
            value: value.clone(),
        })?;
        Ok(Self {
            value,
            project_id,
            number,
        })
    }

    /// Returns the validated two-to-four-letter project ID.
    #[must_use]
    pub fn project_id(&self) -> &ProjectId {
        &self.project_id
    }

    /// Returns the decimal numeric suffix.
    #[must_use]
    pub fn number(&self) -> u16 {
        self.number
    }
}

impl AsRef<str> for TaskId {
    fn as_ref(&self) -> &str {
        &self.value
    }
}

impl std::fmt::Display for TaskId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.value)
    }
}

impl FromStr for TaskId {
    type Err = TaskIdError;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        Self::try_new(map_task_id(raw))
    }
}

fn map_task_id(raw: &str) -> String {
    let trimmed = raw.trim().to_ascii_uppercase();
    let Some(digit_start) = trimmed.find(|character: char| character.is_ascii_digit()) else {
        return trimmed;
    };
    let (code, digits) = trimmed.split_at(digit_start);
    let code = code.strip_suffix('-').unwrap_or(code);
    if !(2..=4).contains(&code.len())
        || !code.chars().all(|character| character.is_ascii_uppercase())
        || digits.is_empty()
        || digits.len() > 4
        || !digits.chars().all(|character| character.is_ascii_digit())
    {
        return trimmed;
    }
    let Ok(number) = digits.parse::<u16>() else {
        return trimmed;
    };
    format!("{code}-{number:04}")
}

/// Reports an invalid full task ID.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("invalid task ID {value:?}")]
pub struct TaskIdError {
    value: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_id_accepts_valid_values() {
        for (raw, number) in [("PW-0047", 47), ("FOO-0047", 47), ("TOOL-9999", 9999)] {
            let id = TaskId::try_new(raw).unwrap();
            assert_eq!(id.as_ref(), raw);
            assert_eq!(id.number(), number);
        }
    }

    #[test]
    fn task_id_from_str_maps_compact_user_input() {
        for (raw, expected) in [
            ("FOO-0098", "FOO-0098"),
            ("foo-0098", "FOO-0098"),
            ("  foo-0047  ", "FOO-0047"),
            ("foo-98", "FOO-0098"),
            ("aux57", "AUX-0057"),
            ("AUX57", "AUX-0057"),
            ("aux-57", "AUX-0057"),
            ("pw7", "PW-0007"),
            ("tool42", "TOOL-0042"),
        ] {
            assert_eq!(raw.parse::<TaskId>().unwrap().as_ref(), expected);
        }
    }

    #[test]
    fn task_id_rejects_invalid_shapes() {
        assert!(TaskId::try_new("foo-0047").is_err());
        assert!(TaskId::try_new("P-0047").is_err());
        assert!(TaskId::try_new("TOOLS-0047").is_err());
        assert!(TaskId::try_new("FOO-47").is_err());
        assert!(TaskId::try_new("aux57").is_err());
        assert!(TaskId::try_new("FOO-0047-extra").is_err());
        assert!(TaskId::try_new("TOOLONG-0047").is_err());
        assert!(TaskId::try_new("AUX-99999").is_err());
        assert!(TaskId::try_new("nope").is_err());
    }
}
