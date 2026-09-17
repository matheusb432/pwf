//! Tokenizes one-line section syntax without intermediate token or word buffers.

use crate::{
    configuration::{MarkerSection, MarkerSectionConfiguration},
    model::ParsedMarkerSections,
    text::single_line,
};

/// Separates consecutive items in the currently selected section.
pub const ITEM_MARKER: &str = "/";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TokenClassification {
    Text,
    ItemMarker,
    ConfiguredSection(usize),
    UnknownSection,
}

fn is_marker_shaped(token: &str) -> bool {
    let bytes = token.as_bytes();
    bytes.len() == 2 && bytes[0] == b'/' && bytes[1].is_ascii_alphabetic()
}

fn classify<const N: usize>(
    token: &str,
    configuration: &MarkerSectionConfiguration<N>,
) -> TokenClassification {
    if token == ITEM_MARKER {
        return TokenClassification::ItemMarker;
    }
    if !is_marker_shaped(token) {
        return TokenClassification::Text;
    }
    match configuration.marker_section(token.as_bytes()[1]) {
        MarkerSection::Configured(index) => TokenClassification::ConfiguredSection(index),
        MarkerSection::Unknown => TokenClassification::UnknownSection,
    }
}

fn push<const N: usize>(
    parsed: &mut ParsedMarkerSections<N>,
    section_index: usize,
    body: &str,
    text_start: Option<usize>,
    text_end: usize,
) {
    let Some(text_start) = text_start else {
        return;
    };
    let text = single_line(&body[text_start..text_end]);
    if !text.is_empty() {
        parsed.section_items[section_index].push(text);
    }
}

/// Parses one-line section syntax using the supplied runtime configuration.
///
/// Text before the first marker is the title. Bare `/` and unknown section-shaped markers start a
/// new item without changing the selected section.
#[must_use]
pub fn parse<const N: usize>(
    body: &str,
    configuration: &MarkerSectionConfiguration<N>,
) -> ParsedMarkerSections<N> {
    debug_assert!(
        N > 0,
        "MarkerSectionConfiguration rejects empty configurations"
    );
    let body_address = body.as_ptr() as usize;
    let mut parsed = ParsedMarkerSections {
        title: String::new(),
        section_items: MarkerSectionConfiguration::<N>::empty_section_items(),
        section_presence: [false; N],
    };
    let mut current_section = 0;
    let mut text_start = None;
    let mut text_end = 0;
    let mut seen_marker = false;

    for token in body.split_whitespace() {
        match classify(token, configuration) {
            TokenClassification::Text => {
                let token_start = token.as_ptr() as usize - body_address;
                text_start.get_or_insert(token_start);
                text_end = token_start + token.len();
            }
            TokenClassification::ItemMarker | TokenClassification::UnknownSection => {
                flush(
                    &mut parsed,
                    current_section,
                    body,
                    &mut text_start,
                    text_end,
                    &mut seen_marker,
                );
            }
            TokenClassification::ConfiguredSection(section_index) => {
                flush(
                    &mut parsed,
                    current_section,
                    body,
                    &mut text_start,
                    text_end,
                    &mut seen_marker,
                );
                current_section = section_index;
                parsed.section_presence[section_index] = true;
            }
        }
    }

    if seen_marker {
        push(&mut parsed, current_section, body, text_start, text_end);
    } else if let Some(text_start) = text_start {
        parsed.title = single_line(&body[text_start..text_end]);
    }
    parsed
}

fn flush<const N: usize>(
    parsed: &mut ParsedMarkerSections<N>,
    current_section: usize,
    body: &str,
    text_start: &mut Option<usize>,
    text_end: usize,
    seen_marker: &mut bool,
) {
    if *seen_marker {
        push(parsed, current_section, body, text_start.take(), text_end);
    } else {
        if let Some(start) = text_start.take() {
            parsed.title = single_line(&body[start..text_end]);
        }
        *seen_marker = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MarkerSectionDefinition;

    fn configuration() -> MarkerSectionConfiguration<4> {
        MarkerSectionConfiguration::try_new([
            MarkerSectionDefinition::try_new("/g", "Goals").unwrap(),
            MarkerSectionDefinition::try_new("/c", "Context").unwrap(),
            MarkerSectionDefinition::try_new("/n", "Constraints").unwrap(),
            MarkerSectionDefinition::try_new("/d", "Done When").unwrap(),
        ])
        .unwrap()
    }

    #[test]
    fn plain_body_sets_only_the_title() {
        let parsed = parse("fix rich body parser", &configuration());
        assert_eq!(parsed.title(), "fix rich body parser");
        assert!(parsed.section_items().iter().all(Vec::is_empty));
    }

    #[test]
    fn marker_first_body_sets_only_the_selected_section() {
        let parsed = parse("/c currently, x does y", &configuration());
        assert!(parsed.title().is_empty());
        assert!(parsed.section_items()[0].is_empty());
        assert_eq!(parsed.section_items()[1], ["currently, x does y"]);
    }

    #[test]
    fn configured_sections_preserve_items_and_encounter_order() {
        let body = "fix rich body parser / preserve ampersands in prose / keep code intact /c current add splits on ampersand /n no parser crate /d tests cover add and update";
        let parsed = parse(body, &configuration());
        assert_eq!(parsed.title(), "fix rich body parser");
        assert_eq!(
            parsed.section_items()[0],
            ["preserve ampersands in prose", "keep code intact"]
        );
        assert_eq!(
            parsed.section_items()[1],
            ["current add splits on ampersand"]
        );
        assert_eq!(parsed.section_items()[2], ["no parser crate"]);
        assert_eq!(parsed.section_items()[3], ["tests cover add and update"]);
    }

    #[test]
    fn bare_marker_continues_the_selected_section() {
        let parsed = parse(
            "title /c context one / context two /d done one / done two",
            &configuration(),
        );
        assert_eq!(parsed.section_items()[1], ["context one", "context two"]);
        assert_eq!(parsed.section_items()[3], ["done one", "done two"]);
    }

    #[test]
    fn sections_can_be_interleaved() {
        let parsed = parse(
            "some title /c some context1 /g another goal /c some context2",
            &configuration(),
        );
        assert_eq!(parsed.section_items()[0], ["another goal"]);
        assert_eq!(
            parsed.section_items()[1],
            ["some context1", "some context2"]
        );
    }

    #[test]
    fn unknown_section_markers_acknowledge_item_boundaries_without_leaking() {
        let body = "alpha beta /x gamma delta /g epsilon zeta /c eta theta /x iota kappa /c lambda mu / nu xi";
        let parsed = parse(body, &configuration());
        assert_eq!(parsed.title(), "alpha beta");
        assert_eq!(parsed.section_items()[0], ["gamma delta", "epsilon zeta"]);
        assert_eq!(
            parsed.section_items()[1],
            ["eta theta", "iota kappa", "lambda mu", "nu xi"]
        );
    }

    #[test]
    fn runtime_markers_replace_compile_time_defaults() {
        let configuration = MarkerSectionConfiguration::try_new([
            MarkerSectionDefinition::try_new("/o", "Objectives").unwrap(),
            MarkerSectionDefinition::try_new("/b", "Background").unwrap(),
        ])
        .unwrap();
        let parsed = parse("title /o first /b second /g third", &configuration);
        assert_eq!(parsed.section_items()[0], ["first"]);
        assert_eq!(parsed.section_items()[1], ["second", "third"]);
    }
}
