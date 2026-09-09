use nutype::nutype;

const DEFAULT_TASK_TITLE: &str = "n/a";
const TASK_TITLE_CHARACTER_LIMIT: usize = 200;
const YAML_UNSAFE_LEADING_CHARACTERS: [char; 16] = [
    ',', '[', ']', '{', '}', '#', '&', '*', '!', '|', '>', '\'', '"', '%', '@', '`',
];

/// Stores a task title.
#[nutype(
    sanitize(with = normalize_task_title),
    validate(len_char_max = TASK_TITLE_CHARACTER_LIMIT),
    derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, AsRef, Display),
    default = DEFAULT_TASK_TITLE,
)]
pub struct TaskTitle(String);

fn normalize_task_title(mut title: String) -> String {
    if title
        .chars()
        .any(|character| !character.to_lowercase().eq(std::iter::once(character)))
    {
        title = title.to_lowercase();
    }
    if task_title_is_normalized(&title) {
        return title;
    }
    let mut collapsed = String::with_capacity(title.len());
    let mut needs_separator = false;
    for character in title.drain(..) {
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

    let safe = yaml_plain_scalar(&collapsed);
    if safe.is_empty() {
        DEFAULT_TASK_TITLE.to_string()
    } else {
        safe
    }
}

fn task_title_is_normalized(title: &str) -> bool {
    if title.is_empty() || strip_unsafe_leading_characters(title) != title || title.ends_with(' ') {
        return false;
    }
    let mut previous_space = false;
    let mut characters = title.chars().peekable();
    while let Some(character) = characters.next() {
        if character.is_whitespace() && (character != ' ' || previous_space) {
            return false;
        }
        if (character == '#' && previous_space)
            || (character == ':' && characters.peek().is_none_or(|next| *next == ' '))
        {
            return false;
        }
        previous_space = character == ' ';
    }
    true
}

fn yaml_plain_scalar(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    let mut characters = title.chars().peekable();
    let mut opens_comment = true;
    while let Some(character) = characters.next() {
        match character {
            ':' => {
                append_colon_run(&mut out, &mut characters);
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

fn append_colon_run(out: &mut String, characters: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    let mut run_length = 1;
    while characters.next_if_eq(&':').is_some() {
        run_length += 1;
    }
    match characters.peek() {
        None => {}
        Some(' ') => out.push(';'),
        Some(_) => out.extend(std::iter::repeat_n(':', run_length)),
    }
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
    fn task_title_normalizes_authored_values() {
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
