use nutype::nutype;

const DEFAULT_TASK_TITLE: &str = "n/a";
const TASK_TITLE_CHARACTER_LIMIT: usize = 200;
const YAML_UNSAFE_LEADING_CHARACTERS: [char; 16] = [
    ',', '[', ']', '{', '}', '#', '&', '*', '!', '|', '>', '\'', '"', '%', '@', '`',
];

#[nutype(
    sanitize(with = canonicalize_task_title),
    validate(len_char_max = TASK_TITLE_CHARACTER_LIMIT),
    derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, AsRef, Display),
)]
pub struct TaskTitle(String);

impl Default for TaskTitle {
    fn default() -> Self {
        Self::try_new(DEFAULT_TASK_TITLE).expect("default task title is valid")
    }
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "nutype custom sanitizers receive the inner String by value"
)]
fn canonicalize_task_title(title: String) -> String {
    let lowercase = title.to_lowercase();
    let safe = yaml_plain_scalar(&lowercase);
    if safe.is_empty() {
        DEFAULT_TASK_TITLE.to_string()
    } else {
        safe
    }
}

fn yaml_plain_scalar(title: &str) -> String {
    let collapsed = title.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out = String::with_capacity(collapsed.len());
    let mut characters = collapsed.chars().peekable();
    let mut opens_comment = true;
    while let Some(character) = characters.next() {
        match character {
            ':' => {
                let mut run_length = 1;
                while characters.next_if_eq(&':').is_some() {
                    run_length += 1;
                }
                match characters.peek() {
                    None => {}
                    Some(' ') => out.push(';'),
                    Some(_) => out.extend(std::iter::repeat_n(':', run_length)),
                }
                opens_comment = false;
            }
            '#' if opens_comment => {}
            _ => {
                out.push(character);
                opens_comment = character == ' ';
            }
        }
    }
    strip_unsafe_leading_characters(out.trim_end()).to_string()
}

fn strip_unsafe_leading_characters(mut value: &str) -> &str {
    loop {
        value = value.trim_start_matches(' ');
        let mut characters = value.chars();
        let Some(first) = characters.next() else {
            return value;
        };
        let unsafe_lead = YAML_UNSAFE_LEADING_CHARACTERS.contains(&first)
            || (matches!(first, '-' | '?' | ':')
                && characters.next().is_none_or(|second| second == ' '));
        if !unsafe_lead {
            return value;
        }
        value = &value[first.len_utf8()..];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_title_canonicalizes_authored_values() {
        for (raw, expected) in [
            ("  Fix Parser: Handle Colons  ", "fix parser; handle colons"),
            ("HUMAN: Do AZ-104", "human; do az-104"),
            ("already lower", "already lower"),
            ("time is 3:30pm", "time is 3:30pm"),
            ("fix foo::bar panic", "fix foo::bar panic"),
            ("read https://docs.rs entry", "read https://docs.rs entry"),
            (
                "finish refactor: promote sync-git seam",
                "finish refactor; promote sync-git seam",
            ),
            ("fix parser:", "fix parser"),
            ("a :: b", "a ; b"),
            ("fix parser :", "fix parser"),
            ("fix #123 now", "fix 123 now"),
            ("# lead hash", "lead hash"),
            ("close c# ticket", "close c# ticket"),
            ("- do it", "do it"),
            ("[wip] fix", "wip] fix"),
            ("\"quoted start", "quoted start"),
            ("? open question", "open question"),
            ("-x marks the spot", "-x marks the spot"),
            ("a\nb: c", "a b; c"),
            ("tab\there", "tab here"),
            (" \n ", "n/a"),
            (":::", "n/a"),
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
