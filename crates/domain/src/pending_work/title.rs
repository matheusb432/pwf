use nutype::nutype;

#[nutype(
    sanitize(trim, with = |raw: String| normalize_title(&raw)),
    validate(not_empty),
    derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, AsRef, Display),
)]
pub struct TaskTitle(String);

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
}
