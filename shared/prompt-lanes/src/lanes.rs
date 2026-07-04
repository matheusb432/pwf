//! Tokenizes the one-line lane syntax into a [`ParsedPrompt`].

use crate::{model::ParsedPrompt, title::single_line};

const PENDING_WORK_TITLE: &str = "pending work";

fn or_pending_work(title: String) -> String {
    if title.is_empty() {
        PENDING_WORK_TITLE.to_string()
    } else {
        title
    }
}

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

/// A `/`-shaped token the caller plainly meant as a lane marker, even if the
/// letter isn't one of the recognized sections — e.g. `/x`. Matches the exact
/// shape of a real marker (`/` + one ASCII letter) so ordinary prose tokens
/// like absolute paths (`/etc/hosts`) are never mistaken for one.
fn is_marker_shaped(token: &str) -> bool {
    let mut chars = token.chars();
    chars.next() == Some('/')
        && chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.next().is_none()
}

/// A token that should flush the current bullet buffer: `/` alone, a
/// recognized section marker, or an unrecognized-but-marker-shaped token
/// (which flushes the buffer without switching section, like bare `/`).
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
    let title = or_pending_work(title);
    ParsedPrompt {
        title: title.clone(),
        goals: vec![title],
        ..ParsedPrompt::default()
    }
}

/// Parses the one-line lane syntax (`title / goal /c context /n constraint /d
/// done when`) into a [`ParsedPrompt`]. A prompt with no lane marker becomes a
/// single-bullet Goals section whose only bullet is the (whitespace-collapsed)
/// prompt itself.
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
            if !seen_first_marker {
                parsed.title = or_pending_work(words_to_text(&buffer));
                parsed.goals.push(parsed.title.clone());
            } else {
                push(&mut parsed, current, words_to_text(&buffer));
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
    fn plain_prompt_makes_title_the_only_goal() {
        let parsed = parse("fix rich prompt parser");
        assert_eq!(parsed.title, "fix rich prompt parser");
        assert_eq!(parsed.goals, vec!["fix rich prompt parser".to_string()]);
        assert!(parsed.context.is_empty());
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
                "fix rich prompt parser".to_string(),
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
        assert_eq!(
            parsed.goals,
            vec!["some title".to_string(), "another goal".to_string()]
        );
        assert_eq!(
            parsed.context,
            vec!["some context1".to_string(), "some context2".to_string()]
        );
    }

    #[test]
    fn ampersands_are_plain_text_to_the_parser() {
        let parsed = parse("handle a & b / preserve c & d");
        assert_eq!(
            parsed.goals,
            vec!["handle a & b".to_string(), "preserve c & d".to_string()]
        );
    }

    #[test]
    fn empty_lead_before_marker_falls_back_to_pending_work() {
        let parsed = parse("/ only second");
        assert_eq!(parsed.title, "pending work");
    }

    #[test]
    fn unrecognized_marker_shaped_token_starts_a_bullet_without_switching_section() {
        // `/x` isn't a known section letter, but it's shaped exactly like one
        // (`/` + one letter) — it must still break the bullet, not get folded
        // into it as literal text.
        let parsed = parse("title /c context one /x context two");
        assert_eq!(
            parsed.context,
            vec!["context one".to_string(), "context two".to_string()]
        );
    }

    #[test]
    fn two_unrecognized_markers_do_not_corrupt_surrounding_bullets() {
        // Regression: a prompt using an unrecognized marker (`/x`) twice, once
        // before the first real marker and once mid-section, used to leak the
        // literal `/x` token into the running text and merge two unrelated
        // clauses into one bullet.
        let prompt = "alpha beta /x gamma delta /g epsilon zeta /c eta theta /x iota kappa /c lambda mu / nu xi";
        let parsed = parse(prompt);
        assert_eq!(
            parsed.goals,
            vec![
                "alpha beta".to_string(),
                "gamma delta".to_string(),
                "epsilon zeta".to_string(),
            ]
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
