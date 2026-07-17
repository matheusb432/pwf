use std::fmt::Write;

use super::Adapter;
use crate::model::ParsedPrompt;

/// Renders a [`ParsedPrompt`] with a required Goals section and non-empty optional sections.
pub struct MarkdownAdapter;

impl Adapter for MarkdownAdapter {
    fn render(&self, parsed: &ParsedPrompt) -> String {
        let mut out = String::from("## Goals");
        for goal in &parsed.goals {
            let _ = write!(out, "\n- {goal}");
        }
        append_section(&mut out, "Context", &parsed.context);
        append_section(&mut out, "Constraints", &parsed.constraints);
        append_section(&mut out, "Done When", &parsed.done_when);
        out
    }
}

fn append_section(out: &mut String, title: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }
    out.push_str("\n\n## ");
    out.push_str(title);
    for item in items {
        let _ = write!(out, "\n- {item}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lanes::parse;

    fn render(prompt: &str) -> String {
        MarkdownAdapter.render(&parse(prompt))
    }

    #[test]
    fn plain_prompt_renders_title_as_the_only_goal() {
        assert_eq!(
            render("fix rich prompt parser"),
            "## Goals\n- fix rich prompt parser"
        );
    }

    #[test]
    fn slash_lanes_render_supported_sections_as_bullets() {
        let prompt = "fix rich prompt parser / preserve ampersands in prose / keep code intact /c current add splits on ampersand /n no parser crate /d tests cover add and update";
        assert_eq!(
            render(prompt),
            "## Goals\n- fix rich prompt parser\n- preserve ampersands in prose\n- keep code intact\n\n## Context\n- current add splits on ampersand\n\n## Constraints\n- no parser crate\n\n## Done When\n- tests cover add and update"
        );
    }

    #[test]
    fn standalone_slash_continues_the_current_section() {
        assert_eq!(
            render("title /c context one / context two /d done one / done two"),
            "## Goals\n- title\n\n## Context\n- context one\n- context two\n\n## Done When\n- done one\n- done two"
        );
    }

    #[test]
    fn sections_can_be_interleaved_and_append_in_encounter_order() {
        assert_eq!(
            render("some title /c some context1 /g another goal /c some context2"),
            "## Goals\n- some title\n- another goal\n\n## Context\n- some context1\n- some context2"
        );
    }

    #[test]
    fn ampersands_are_plain_text_to_the_prompt_parser() {
        assert_eq!(
            render("handle a & b / preserve c & d"),
            "## Goals\n- handle a & b\n- preserve c & d"
        );
    }
}
