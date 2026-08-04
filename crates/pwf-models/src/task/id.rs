use std::str::FromStr;

use nutype::nutype;

use crate::project::ProjectId;

#[nutype(
    validate(predicate = is_canonical_task_id),
    derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, AsRef, Display,)
)]
pub struct TaskId(String);

impl TaskId {
    /// Returns the validated three-letter project prefix.
    ///
    /// # Panics
    ///
    /// Panics only if the `TaskId` invariant was bypassed internally.
    pub fn project_id(&self) -> ProjectId {
        let (code, _) = self
            .as_ref()
            .split_once('-')
            .expect("TaskId validation requires a separator");
        ProjectId::try_new(code).expect("TaskId validation requires a canonical project ID")
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

fn is_canonical_task_id(raw: &str) -> bool {
    let Some((code, digits)) = raw.split_once('-') else {
        return false;
    };
    let Ok(project_id) = ProjectId::try_new(code) else {
        return false;
    };
    project_id.as_ref() == code && digits.len() == 4 && digits.chars().all(|ch| ch.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_id_accepts_canonical_values() {
        let id = TaskId::try_new("PWF-0047").unwrap();

        assert_eq!(id.as_ref(), "PWF-0047");
        assert_eq!(id.project_id(), ProjectId::try_new("PWF").unwrap());
    }

    #[test]
    fn task_id_from_str_maps_compact_user_input() {
        for (raw, expected) in [
            ("PWF-0098", "PWF-0098"),
            ("pwf-0098", "PWF-0098"),
            ("  pwf-0047  ", "PWF-0047"),
            ("pwf-98", "PWF-0098"),
            ("cfg57", "CFG-0057"),
            ("CFG57", "CFG-0057"),
            ("cfg-57", "CFG-0057"),
        ] {
            assert_eq!(raw.parse::<TaskId>().unwrap().as_ref(), expected);
        }
    }

    #[test]
    fn task_id_rejects_noncanonical_shapes() {
        assert!(TaskId::try_new("pwf-0047").is_err());
        assert!(TaskId::try_new("PW-0047").is_err());
        assert!(TaskId::try_new("TOOL-0047").is_err());
        assert!(TaskId::try_new("PWF-47").is_err());
        assert!(TaskId::try_new("cfg57").is_err());
        assert!(TaskId::try_new("PWF-0047-extra").is_err());
        assert!(TaskId::try_new("TOOLONG-0047").is_err());
        assert!(TaskId::try_new("CFG-99999").is_err());
        assert!(TaskId::try_new("nope").is_err());
    }
}
