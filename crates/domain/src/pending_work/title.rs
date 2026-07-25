use nutype::nutype;
const DEFAULT_TASK_TITLE: &str = "n/a";

#[nutype(
    validate(predicate = is_canonical_task_title),
    derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, AsRef, Display),
)]
pub struct TaskTitle(String);

impl Default for TaskTitle {
    fn default() -> Self {
        Self::try_new(DEFAULT_TASK_TITLE).expect("default task title is valid")
    }
}

fn is_canonical_task_title(title: &str) -> bool {
    !title.is_empty() && title.trim() == title && title.to_lowercase() == title
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_title_accepts_only_canonical_values() {
        assert_eq!(
            TaskTitle::try_new("fix the thing").unwrap().as_ref(),
            "fix the thing"
        );
        assert!(TaskTitle::try_new("Fix The THING").is_err());
        assert!(TaskTitle::try_new("  fix the thing  ").is_err());
        assert!(TaskTitle::try_new(" \n ").is_err());
    }

    #[test]
    fn missing_task_title_defaults_to_validated_na_value() {
        let title = TaskTitle::default();

        assert_eq!(title.as_ref(), "n/a");
        assert_eq!(TaskTitle::try_new(title.to_string()).unwrap(), title);
    }
}
