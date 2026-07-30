//! Tokenizes the one-line lane syntax into a [`ParsedPrompt`].

use crate::{model::ParsedPrompt, title::single_line};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    Goals,
    Context,
    Constraints,
    DoneWhen,
}

impl Section {
    fn from_marker(token: &str) -> Option<Self> {
        match token {
            "/g" => Some(Self::Goals),
            "/c" => Some(Self::Context),
            "/n" => Some(Self::Constraints),
            "/d" => Some(Self::DoneWhen),
            _ => None,
        }
    }
}

/// Reports whether a token has the `/` plus one ASCII letter marker shape.
///
/// This recognizes unknown markers such as `/x` without treating paths such as `/etc/hosts` as
/// markers.
fn is_marker_shaped(token: &str) -> bool {
    let mut chars = token.chars();
    chars.next() == Some('/')
        && chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.next().is_none()
}

/// Reports whether a token flushes the current bullet buffer.
///
/// Bare `/` and unknown marker-shaped tokens flush without selecting a new section.
fn is_marker(token: &str) -> bool {
    token == "/" || Section::from_marker(token).is_some() || is_marker_shaped(token)
}

fn words_to_text(words: &[&str]) -> String {
    words.join(" ")
}

fn push(parsed: &mut ParsedPrompt, section: Section, text: String) {
    if text.is_empty() {
        return;
    }
    match section {
        Section::Goals => parsed.goals.push(text),
        Section::Context => parsed.context.push(text),
        Section::Constraints => parsed.constraints.push(text),
        Section::DoneWhen => parsed.done_when.push(text),
    }
}

fn plain(prompt: &str) -> ParsedPrompt {
    let title = single_line(prompt);
    ParsedPrompt {
        title,
        ..ParsedPrompt::default()
    }
}

/// Parses one-line lane syntax into a [`ParsedPrompt`].
///
/// Text before the first lane marker is the title and is not copied into Goals.
pub fn parse(prompt: &str) -> ParsedPrompt {
    let tokens: Vec<&str> = prompt.split_whitespace().collect();
    if !tokens.iter().any(|token| is_marker(token)) {
        return plain(prompt);
    }

    let mut parsed = ParsedPrompt::default();
    let mut current = Section::Goals;
    let mut buffer = Vec::new();
    let mut seen_first_marker = false;

    for token in tokens {
        if is_marker(token) {
            if seen_first_marker {
                push(&mut parsed, current, words_to_text(&buffer));
            } else {
                let title = words_to_text(&buffer);
                parsed.title.clone_from(&title);
            }
            buffer.clear();
            seen_first_marker = true;
            if let Some(section) = Section::from_marker(token) {
                current = section;
            }
        } else {
            buffer.push(token);
        }
    }

    push(&mut parsed, current, words_to_text(&buffer));
    parsed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_prompt_sets_the_title_without_repeating_it_as_a_goal() {
        let parsed = parse("fix rich prompt parser");
        assert_eq!(parsed.title, "fix rich prompt parser");
        assert!(parsed.goals.is_empty());
        assert!(parsed.context.is_empty());
        assert!(parsed.constraints.is_empty());
        assert!(parsed.done_when.is_empty());
    }

    #[test]
    fn plain_prompt_with_section_start_appends_only_section() {
        let parsed = parse("/c currently, x does y");
        assert!(parsed.title.is_empty());
        assert!(parsed.goals.is_empty());
        assert_eq!(parsed.context, vec!["currently, x does y".to_string()]);
        assert!(parsed.constraints.is_empty());
        assert!(parsed.done_when.is_empty());
    }

    #[test]
    fn slash_lanes_populate_each_section() {
        let prompt = "fix rich prompt parser / preserve ampersands in prose / keep code intact /c current add splits on ampersand /n no parser crate /d tests cover add and update";
        let parsed = parse(prompt);
        assert_eq!(parsed.title, "fix rich prompt parser");
        assert_eq!(
            parsed.goals,
            vec![
                "preserve ampersands in prose".to_string(),
                "keep code intact".to_string(),
            ]
        );
        assert_eq!(
            parsed.context,
            vec!["current add splits on ampersand".to_string()]
        );
        assert_eq!(parsed.constraints, vec!["no parser crate".to_string()]);
        assert_eq!(
            parsed.done_when,
            vec!["tests cover add and update".to_string()]
        );
    }

    #[test]
    fn standalone_slash_continues_the_current_section() {
        let parsed = parse("title /c context one / context two /d done one / done two");
        assert!(parsed.goals.is_empty());
        assert_eq!(
            parsed.context,
            vec!["context one".to_string(), "context two".to_string()]
        );
        assert_eq!(
            parsed.done_when,
            vec!["done one".to_string(), "done two".to_string()]
        );
    }

    #[test]
    fn sections_can_be_interleaved_and_append_in_encounter_order() {
        let parsed = parse("some title /c some context1 /g another goal /c some context2");
        assert_eq!(parsed.goals, vec!["another goal".to_string()]);
        assert_eq!(
            parsed.context,
            vec!["some context1".to_string(), "some context2".to_string()]
        );
    }

    #[test]
    fn ampersands_are_plain_text_to_the_parser() {
        let parsed = parse("handle a & b / preserve c & d");
        assert_eq!(parsed.goals, vec!["preserve c & d".to_string()]);
    }

    #[test]
    fn empty_lead_before_marker_emits_only_authored_content() {
        let parsed = parse("/ only second");
        assert!(parsed.title.is_empty());
        assert_eq!(parsed.goals, vec!["only second".to_string()]);
    }

    #[test]
    fn unrecognized_marker_shaped_token_starts_a_bullet_without_switching_section() {
        let parsed = parse("title /c context one /x context two");
        assert_eq!(
            parsed.context,
            vec!["context one".to_string(), "context two".to_string()]
        );
    }

    #[test]
    fn two_unrecognized_markers_do_not_corrupt_surrounding_bullets() {
        let prompt = "alpha beta /x gamma delta /g epsilon zeta /c eta theta /x iota kappa /c lambda mu / nu xi";
        let parsed = parse(prompt);
        assert_eq!(
            parsed.goals,
            vec!["gamma delta".to_string(), "epsilon zeta".to_string(),]
        );
        assert_eq!(
            parsed.context,
            vec![
                "eta theta".to_string(),
                "iota kappa".to_string(),
                "lambda mu".to_string(),
                "nu xi".to_string(),
            ]
        );
        for bullet in parsed.goals.iter().chain(parsed.context.iter()) {
            assert!(!bullet.contains('/'), "marker leaked into bullet: {bullet}");
        }
    }
}
