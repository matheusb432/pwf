use nutype::nutype;

const DEFAULT_TASK_TITLE: &str = "n/a";

#[nutype(
    sanitize(trim, with = |raw: String| normalize_title(&raw)),
    validate(not_empty),
    derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, AsRef, Display),
)]
pub struct TaskTitle(String);

impl Default for TaskTitle {
    fn default() -> Self {
        Self::try_new(DEFAULT_TASK_TITLE).expect("default task title is valid")
    }
}

fn normalize_title(title: &str) -> String {
    title.to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_title_trims_lowercases_and_rejects_blank() {
        assert_eq!(
            TaskTitle::try_new("  Fix The THING  ").unwrap().as_ref(),
            "fix the thing"
        );
        assert!(TaskTitle::try_new(" \n ").is_err());
    }

    #[test]
    fn missing_task_title_defaults_to_validated_na_value() {
        let title = TaskTitle::default();

        assert_eq!(title.as_ref(), "n/a");
        assert_eq!(TaskTitle::try_new(title.to_string()).unwrap(), title);
    }
}
