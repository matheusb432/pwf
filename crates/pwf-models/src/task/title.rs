use nutype::nutype;

const DEFAULT_TASK_TITLE: &str = "n/a";
const TASK_TITLE_CHARACTER_LIMIT: usize = 200;

/// Stores a task title with normalized whitespace and its original case and punctuation.
#[nutype(
    sanitize(with = normalize_task_title),
    validate(len_char_max = TASK_TITLE_CHARACTER_LIMIT),
    derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, AsRef, Display),
    default = DEFAULT_TASK_TITLE,
)]
pub struct TaskTitle(String);

fn normalize_task_title(title: String) -> String {
    if task_title_is_normalized(&title) {
        return title;
    }
    let mut collapsed = String::with_capacity(title.len());
    let mut needs_separator = false;
    for character in title.chars() {
        if character.is_whitespace() {
            needs_separator = !collapsed.is_empty();
            continue;
        }
        if needs_separator {
            collapsed.push(' ');
            needs_separator = false;
        }
        collapsed.push(character);
    }

    if collapsed.is_empty() {
        DEFAULT_TASK_TITLE.to_string()
    } else {
        collapsed
    }
}

fn task_title_is_normalized(title: &str) -> bool {
    if title.is_empty() || title.starts_with(' ') || title.ends_with(' ') {
        return false;
    }
    let mut previous_space = false;
    for character in title.chars() {
        if character.is_whitespace() && (character != ' ' || previous_space) {
            return false;
        }
        previous_space = character == ' ';
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_title_preserves_authored_text_and_normalizes_whitespace() {
        for (raw, expected) in [
            ("  Fix Parser: Handle Colons  ", "Fix Parser: Handle Colons"),
            ("HUMAN: Do AZ-104", "HUMAN: Do AZ-104"),
            ("already lower", "already lower"),
            ("time is 3:30pm", "time is 3:30pm"),
            ("fix foo::bar panic", "fix foo::bar panic"),
            ("read https://docs.rs entry", "read https://docs.rs entry"),
            (
                "finish refactor: promote sync-git seam",
                "finish refactor: promote sync-git seam",
            ),
            ("fix parser:", "fix parser:"),
            ("a :: b", "a :: b"),
            ("fix parser :", "fix parser :"),
            ("fix #123 now", "fix #123 now"),
            ("# lead hash", "# lead hash"),
            ("close c# ticket", "close c# ticket"),
            ("- do it", "- do it"),
            ("[wip] fix", "[wip] fix"),
            ("\"quoted start", "\"quoted start"),
            ("? open question", "? open question"),
            ("-x marks the spot", "-x marks the spot"),
            ("a\nb: c", "a b: c"),
            ("tab\there", "tab here"),
            ("Unicode: ação; 日本語", "Unicode: ação; 日本語"),
            (" \n ", "n/a"),
            (":::", ":::"),
        ] {
            assert_eq!(TaskTitle::try_new(raw).unwrap().as_ref(), expected);
        }
    }

    #[test]
    fn task_title_enforces_a_200_character_limit() {
        let accepted = "\u{e9}".repeat(200);
        assert_eq!(
            TaskTitle::try_new(&accepted)
                .unwrap()
                .as_ref()
                .chars()
                .count(),
            200
        );

        let error = TaskTitle::try_new("\u{e9}".repeat(201)).unwrap_err();
        assert_eq!(
            error.to_string(),
            "TaskTitle is too long: the maximum valid length is 200 characters."
        );
    }

    #[test]
    fn missing_task_title_defaults_to_validated_na_value() {
        let title = TaskTitle::default();

        assert_eq!(title.as_ref(), "n/a");
        assert_eq!(TaskTitle::try_new(title.to_string()).unwrap(), title);
    }
}
