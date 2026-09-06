//! Tokenizes one-line lane syntax without intermediate token or word buffers.

use crate::{
    configuration::{LaneConfiguration, MarkerLane},
    model::ParsedPrompt,
    text::single_line,
};

/// Separates consecutive items in the currently selected lane.
pub const ITEM_MARKER: &str = "/";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TokenClassification {
    Text,
    ItemMarker,
    ConfiguredLane(usize),
    UnknownLane,
}

fn is_lane_marker_shaped(token: &str) -> bool {
    let bytes = token.as_bytes();
    bytes.len() == 2 && bytes[0] == b'/' && bytes[1].is_ascii_alphabetic()
}

fn classify<const N: usize>(
    token: &str,
    configuration: &LaneConfiguration<N>,
) -> TokenClassification {
    if token == ITEM_MARKER {
        return TokenClassification::ItemMarker;
    }
    if !is_lane_marker_shaped(token) {
        return TokenClassification::Text;
    }
    match configuration.marker_lane(token.as_bytes()[1]) {
        MarkerLane::Configured(index) => TokenClassification::ConfiguredLane(index),
        MarkerLane::Unknown => TokenClassification::UnknownLane,
    }
}

fn push<const N: usize>(
    parsed: &mut ParsedPrompt<N>,
    lane_index: usize,
    prompt: &str,
    text_start: Option<usize>,
    text_end: usize,
) {
    let Some(text_start) = text_start else {
        return;
    };
    let text = single_line(&prompt[text_start..text_end]);
    if !text.is_empty() {
        parsed.lane_items[lane_index].push(text);
    }
}

/// Parses one-line lane syntax using the supplied runtime configuration.
///
/// Text before the first marker is the title. Bare `/` and unknown lane-shaped markers start a
/// new item without changing the selected lane.
#[must_use]
pub fn parse<const N: usize>(
    prompt: &str,
    configuration: &LaneConfiguration<N>,
) -> ParsedPrompt<N> {
    debug_assert!(N > 0, "LaneConfiguration rejects empty configurations");
    let prompt_address = prompt.as_ptr() as usize;
    let mut parsed = ParsedPrompt {
        title: String::new(),
        lane_items: LaneConfiguration::<N>::empty_lane_items(),
    };
    let mut current_lane = 0;
    let mut text_start = None;
    let mut text_end = 0;
    let mut seen_marker = false;

    for token in prompt.split_whitespace() {
        match classify(token, configuration) {
            TokenClassification::Text => {
                let token_start = token.as_ptr() as usize - prompt_address;
                text_start.get_or_insert(token_start);
                text_end = token_start + token.len();
            }
            TokenClassification::ItemMarker | TokenClassification::UnknownLane => {
                flush(
                    &mut parsed,
                    current_lane,
                    prompt,
                    &mut text_start,
                    text_end,
                    &mut seen_marker,
                );
            }
            TokenClassification::ConfiguredLane(lane_index) => {
                flush(
                    &mut parsed,
                    current_lane,
                    prompt,
                    &mut text_start,
                    text_end,
                    &mut seen_marker,
                );
                current_lane = lane_index;
            }
        }
    }

    if seen_marker {
        push(&mut parsed, current_lane, prompt, text_start, text_end);
    } else if let Some(text_start) = text_start {
        parsed.title = single_line(&prompt[text_start..text_end]);
    }
    parsed
}

fn flush<const N: usize>(
    parsed: &mut ParsedPrompt<N>,
    current_lane: usize,
    prompt: &str,
    text_start: &mut Option<usize>,
    text_end: usize,
    seen_marker: &mut bool,
) {
    if *seen_marker {
        push(parsed, current_lane, prompt, text_start.take(), text_end);
    } else {
        if let Some(start) = text_start.take() {
            parsed.title = single_line(&prompt[start..text_end]);
        }
        *seen_marker = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LaneDefinition;

    fn configuration() -> LaneConfiguration<4> {
        LaneConfiguration::try_new([
            LaneDefinition::try_new("/g", "Goals").unwrap(),
            LaneDefinition::try_new("/c", "Context").unwrap(),
            LaneDefinition::try_new("/n", "Constraints").unwrap(),
            LaneDefinition::try_new("/d", "Done When").unwrap(),
        ])
        .unwrap()
    }

    #[test]
    fn plain_prompt_sets_only_the_title() {
        let parsed = parse("fix rich prompt parser", &configuration());
        assert_eq!(parsed.title(), "fix rich prompt parser");
        assert!(parsed.lane_items().iter().all(Vec::is_empty));
    }

    #[test]
    fn marker_first_prompt_sets_only_the_selected_lane() {
        let parsed = parse("/c currently, x does y", &configuration());
        assert!(parsed.title().is_empty());
        assert!(parsed.lane_items()[0].is_empty());
        assert_eq!(parsed.lane_items()[1], ["currently, x does y"]);
    }

    #[test]
    fn configured_lanes_preserve_items_and_encounter_order() {
        let prompt = "fix rich prompt parser / preserve ampersands in prose / keep code intact /c current add splits on ampersand /n no parser crate /d tests cover add and update";
        let parsed = parse(prompt, &configuration());
        assert_eq!(parsed.title(), "fix rich prompt parser");
        assert_eq!(
            parsed.lane_items()[0],
            ["preserve ampersands in prose", "keep code intact"]
        );
        assert_eq!(parsed.lane_items()[1], ["current add splits on ampersand"]);
        assert_eq!(parsed.lane_items()[2], ["no parser crate"]);
        assert_eq!(parsed.lane_items()[3], ["tests cover add and update"]);
    }

    #[test]
    fn bare_marker_continues_the_selected_lane() {
        let parsed = parse(
            "title /c context one / context two /d done one / done two",
            &configuration(),
        );
        assert_eq!(parsed.lane_items()[1], ["context one", "context two"]);
        assert_eq!(parsed.lane_items()[3], ["done one", "done two"]);
    }

    #[test]
    fn lanes_can_be_interleaved() {
        let parsed = parse(
            "some title /c some context1 /g another goal /c some context2",
            &configuration(),
        );
        assert_eq!(parsed.lane_items()[0], ["another goal"]);
        assert_eq!(parsed.lane_items()[1], ["some context1", "some context2"]);
    }

    #[test]
    fn unknown_lane_markers_acknowledge_item_boundaries_without_leaking() {
        let prompt = "alpha beta /x gamma delta /g epsilon zeta /c eta theta /x iota kappa /c lambda mu / nu xi";
        let parsed = parse(prompt, &configuration());
        assert_eq!(parsed.title(), "alpha beta");
        assert_eq!(parsed.lane_items()[0], ["gamma delta", "epsilon zeta"]);
        assert_eq!(
            parsed.lane_items()[1],
            ["eta theta", "iota kappa", "lambda mu", "nu xi"]
        );
    }

    #[test]
    fn runtime_markers_replace_compile_time_defaults() {
        let configuration = LaneConfiguration::try_new([
            LaneDefinition::try_new("/o", "Objectives").unwrap(),
            LaneDefinition::try_new("/b", "Background").unwrap(),
        ])
        .unwrap();
        let parsed = parse("title /o first /b second /g third", &configuration);
        assert_eq!(parsed.lane_items()[0], ["first"]);
        assert_eq!(parsed.lane_items()[1], ["second", "third"]);
    }
}
