use std::fmt::Write;

use super::Adapter;
use crate::model::ParsedPrompt;

/// Renders a [`ParsedPrompt`] with a required Goals section and non-empty optional sections.
pub struct MarkdownAdapter;

impl Adapter for MarkdownAdapter {
    fn render(&self, parsed: &ParsedPrompt) -> String {
        // FIXME: optimize (can prealloc all mem easily since `parsed` already tells us how big this
        // gets)
        let mut out = String::from("## Goals\n");
        for goal in &parsed.goals {
            let _ = write!(out, "\n- {goal}");
        }
        append_section(&mut out, "Context", &parsed.context);
        append_section(&mut out, "Constraints", &parsed.constraints);
        append_section(&mut out, "Done When", &parsed.done_when);
        out
    }
}

// FIXME: needless indirection in `items`, since this method always moves the Strings if they exist.
// can optimize it (but profile it first to measure actual gains!)
fn append_section(out: &mut String, title: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }
    out.push_str("\n\n## ");
    out.push_str(title);
    out.push('\n');
    for item in items {
        let _ = write!(out, "\n- {item}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lanes::parse;

    const S: &str = "\n\n";

    fn render(prompt: &str) -> String {
        MarkdownAdapter.render(&parse(prompt))
    }

    #[test]
    fn plain_prompt_renders_title_as_the_only_goal() {
        assert_eq!(
            render("fix rich prompt parser"),
            format!("## Goals{S}- fix rich prompt parser")
        );
    }

    #[test]
    fn slash_lanes_render_supported_sections_as_bullets() {
        let prompt = "fix rich prompt parser / preserve ampersands in prose / keep code intact /c current add splits on ampersand /n no parser crate /d tests cover add and update";
        assert_eq!(
            render(prompt),
            format!(
                "## Goals{S}- fix rich prompt parser\n- preserve ampersands in prose\n- keep code intact{S}## Context{S}- current add splits on ampersand{S}## Constraints{S}- no parser crate{S}## Done When{S}- tests cover add and update"
            )
        );
    }

    #[test]
    fn standalone_slash_continues_the_current_section() {
        assert_eq!(
            render("title /c context one / context two /d done one / done two"),
            format!(
                "## Goals{S}- title{S}## Context{S}- context one\n- context two{S}## Done When{S}- done one\n- done two"
            )
        );
    }

    #[test]
    fn sections_can_be_interleaved_and_append_in_encounter_order() {
        assert_eq!(
            render("some title /c some context1 /g another goal /c some context2"),
            format!(
                "## Goals{S}- some title\n- another goal{S}## Context{S}- some context1\n- some context2"
            )
        );
    }

    #[test]
    fn ampersands_are_plain_text_to_the_prompt_parser() {
        assert_eq!(
            render("handle a & b / preserve c & d"),
            format!("## Goals{S}- handle a & b\n- preserve c & d")
        );
    }
}
