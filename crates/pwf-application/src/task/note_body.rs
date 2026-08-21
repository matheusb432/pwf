//! Applies prompt, lane, and report transforms to task note bodies.

use lazy_regex::{Regex, regex};
use prompt_lanes::{Adapter, MarkdownAdapter, ParsedPrompt, parse};
use pwf_models::task::TaskPrompt;

use crate::contract::task::{TaskLane, TaskLaneEdits, TaskLanes};

const REPORT_SEPARATOR: &str = "\n\n### Report\n\n";
const LANE_SECTION_HEADERS: [&str; 4] =
    ["## Goals", "## Context", "## Constraints", "## Done When"];

fn placeholder_prompt_regex() -> &'static Regex {
    regex!(r"(?i)(^\s*\[!\]\s*TODO\b|^\s*TODO\b|definir prompt|define prompt|tbd)")
}

enum PromptClassification {
    Placeholder,
    AuthoredVerbatimLegacy,
    Authored,
}

#[must_use]
pub(in crate::task) fn render(prompt: &TaskPrompt) -> String {
    match prompt_classification(prompt) {
        PromptClassification::Placeholder | PromptClassification::AuthoredVerbatimLegacy => {
            prompt.to_string()
        }
        PromptClassification::Authored => MarkdownAdapter.render(&parse(prompt.as_ref())),
    }
}

#[must_use]
pub(in crate::task) fn render_lanes(lanes: &TaskLanes) -> String {
    MarkdownAdapter.render(&ParsedPrompt {
        title: String::new(),
        goals: lanes.goals().to_vec(),
        context: lanes.context().to_vec(),
        constraints: lanes.constraints().to_vec(),
        done_when: lanes.done_when().to_vec(),
    })
}

/// Reports whether a prompt is empty or matches `TODO`, `[!] TODO`, `define prompt`, `definir
/// prompt`, or `tbd` case-insensitively.
#[must_use]
pub(in crate::task) fn is_placeholder_prompt(prompt: &TaskPrompt) -> bool {
    matches!(
        prompt_classification(prompt),
        PromptClassification::Placeholder
    )
}

fn prompt_classification(prompt: &TaskPrompt) -> PromptClassification {
    let prompt = prompt.as_ref();
    if prompt.trim().is_empty() || placeholder_prompt_regex().is_match(prompt) {
        return PromptClassification::Placeholder;
    }

    // Rendering uses ASCII boundaries, but diagnostics use Unicode boundaries.
    let prompt_lowercase = prompt.to_lowercase();
    let prompt_trimmed = prompt_lowercase.trim_start();
    let prompt_after_marker = prompt_trimmed.strip_prefix("[!]").map(str::trim_start);
    if [Some(prompt_trimmed), prompt_after_marker]
        .into_iter()
        .flatten()
        .any(starts_with_todo_word_boundary_ascii)
    {
        PromptClassification::AuthoredVerbatimLegacy
    } else {
        PromptClassification::Authored
    }
}

fn starts_with_todo_word_boundary_ascii(text: &str) -> bool {
    text.strip_prefix("todo").is_some_and(|rest| {
        !rest.starts_with(|character: char| character.is_ascii_alphanumeric() || character == '_')
    })
}

/// Splices lane bullets into existing sections and appends missing sections.
///
/// Returns [`None`] for a whitespace-only prompt.
#[must_use]
pub(in crate::task) fn append_lanes(body: &str, prompt: &TaskPrompt) -> String {
    let prompt = prompt.as_ref().trim();
    debug_assert!(!prompt.is_empty(), "task append prompts are validated");
    let mut parsed = parse(prompt);
    if !parsed.title.is_empty() {
        parsed.goals.insert(0, std::mem::take(&mut parsed.title));
    }
    let sections: [&[String]; 4] = [
        &parsed.goals,
        &parsed.context,
        &parsed.constraints,
        &parsed.done_when,
    ];
    let mut out = body.to_string();
    for (header, bullets) in LANE_SECTION_HEADERS.iter().zip(sections) {
        out = append_bullets_to_section(&out, header, bullets);
    }
    out
}

#[derive(Debug, Clone, Copy, thiserror::Error)]
pub(in crate::task) enum EditLanesError {
    #[error("task body contains more than one `{header}` section")]
    DuplicateSection { header: &'static str },
}

#[derive(Debug, Clone, Copy)]
enum LaneSection {
    Goals,
    Context,
    Constraints,
    DoneWhen,
}

impl LaneSection {
    const ALL: [Self; 4] = [
        Self::Goals,
        Self::Context,
        Self::Constraints,
        Self::DoneWhen,
    ];

    fn header(self) -> &'static str {
        match self {
            Self::Goals => "## Goals",
            Self::Context => "## Context",
            Self::Constraints => "## Constraints",
            Self::DoneWhen => "## Done When",
        }
    }

    fn index(self) -> usize {
        match self {
            Self::Goals => 0,
            Self::Context => 1,
            Self::Constraints => 2,
            Self::DoneWhen => 3,
        }
    }
}

pub(in crate::task) fn edit_lanes(
    body: &str,
    edits: &TaskLaneEdits,
) -> Result<Option<String>, EditLanesError> {
    reject_duplicate_lane_sections(body)?;
    let mut edited = body.to_string();

    for (section, remove) in [
        (
            LaneSection::Goals,
            edits.removals().contains(&TaskLane::Goal),
        ),
        (
            LaneSection::Context,
            edits.removals().contains(&TaskLane::Context),
        ),
        (
            LaneSection::Constraints,
            edits.removals().contains(&TaskLane::Constraint),
        ),
        (
            LaneSection::DoneWhen,
            edits.removals().contains(&TaskLane::DoneWhen),
        ),
    ] {
        if remove {
            edited = clear_lane_section(&edited, section);
        }
    }

    for (section, values) in [
        (LaneSection::Goals, edits.additions().goals()),
        (LaneSection::Context, edits.additions().context()),
        (LaneSection::Constraints, edits.additions().constraints()),
        (LaneSection::DoneWhen, edits.additions().done_when()),
    ] {
        if !values.is_empty() {
            edited = add_lane_values(&edited, section, values);
        }
    }

    Ok((edited != body).then_some(edited))
}

fn reject_duplicate_lane_sections(body: &str) -> Result<(), EditLanesError> {
    for section in LaneSection::ALL {
        if header_bounds(body, section.header()).len() > 1 {
            return Err(EditLanesError::DuplicateSection {
                header: section.header(),
            });
        }
    }
    Ok(())
}

fn clear_lane_section(body: &str, section: LaneSection) -> String {
    let Some((header_start, header_end)) = header_bounds(body, section.header()).into_iter().next()
    else {
        return if matches!(section, LaneSection::Goals) {
            insert_lane_section(body, section, &[])
        } else {
            body.to_string()
        };
    };
    let section_end = section_end(body, header_end);
    let replacement = matches!(section, LaneSection::Goals).then_some(section.header());
    replace_region(body, header_start, section_end, replacement)
}

fn add_lane_values(body: &str, section: LaneSection, values: &[String]) -> String {
    if header_bounds(body, section.header()).is_empty() {
        insert_lane_section(body, section, values)
    } else {
        append_bullets_to_section(body, section.header(), values)
    }
}

fn insert_lane_section(body: &str, section: LaneSection, values: &[String]) -> String {
    let insertion_offset = LaneSection::ALL
        .into_iter()
        .filter(|candidate| candidate.index() > section.index())
        .flat_map(|candidate| header_bounds(body, candidate.header()))
        .map(|(start, _)| start)
        .min()
        .unwrap_or(body.len());
    let mut inserted = section.header().to_string();
    for (index, value) in values.iter().enumerate() {
        inserted.push_str(if index == 0 { "\n\n- " } else { "\n- " });
        inserted.push_str(value);
    }
    replace_region(body, insertion_offset, insertion_offset, Some(&inserted))
}

fn replace_region(body: &str, start: usize, end: usize, replacement: Option<&str>) -> String {
    let before = body[..start].trim_end_matches('\n');
    let after = body[end..].trim_start_matches('\n');
    let mut parts = Vec::with_capacity(3);
    if !before.is_empty() {
        parts.push(before);
    }
    if let Some(replacement) = replacement {
        parts.push(replacement.trim_matches('\n'));
    }
    if !after.is_empty() {
        parts.push(after);
    }
    parts.join("\n\n")
}

fn header_bounds(content: &str, header: &str) -> Vec<(usize, usize)> {
    let mut bounds = Vec::new();
    let mut offset = 0;
    for segment in content.split('\n') {
        if segment
            .strip_prefix(header)
            .is_some_and(|rest| rest.trim().is_empty())
        {
            bounds.push((offset, offset + segment.len()));
        }
        offset += segment.len() + 1;
    }
    bounds
}

fn section_end(content: &str, header_end: usize) -> usize {
    let rest = &content[header_end..];
    header_end + next_heading_offset(rest).unwrap_or(rest.len())
}

fn append_bullets_to_section(content: &str, header: &str, bullets: &[String]) -> String {
    if bullets.is_empty() {
        return content.to_string();
    }
    let Some(header_end) = header_line_end(content, header) else {
        let mut out = content.trim_end().to_string();
        out.push_str("\n\n");
        out.push_str(header);
        for (index, bullet) in bullets.iter().enumerate() {
            out.push_str(if index == 0 { "\n\n- " } else { "\n- " });
            out.push_str(bullet);
        }
        out.push('\n');
        return out;
    };
    let rest = &content[header_end..];
    let section_end = header_end + next_heading_offset(rest).unwrap_or(rest.len());
    let before = content[..section_end].trim_end_matches('\n');
    let after = content[section_end..].trim_start_matches('\n');
    let mut out = before.to_string();
    let section_is_empty = content[header_end..section_end].trim().is_empty();
    for (index, bullet) in bullets.iter().enumerate() {
        out.push_str(if section_is_empty && index == 0 {
            "\n\n- "
        } else {
            "\n- "
        });
        out.push_str(bullet);
    }
    if after.is_empty() {
        out.push('\n');
    } else {
        out.push_str("\n\n");
        out.push_str(after);
    }
    out
}

/// Returns the byte offset after an exact header line, allowing trailing whitespace.
fn header_line_end(content: &str, header: &str) -> Option<usize> {
    let mut offset = 0;
    for segment in content.split('\n') {
        if segment
            .strip_prefix(header)
            .is_some_and(|rest| rest.trim().is_empty())
        {
            return Some(offset + segment.len());
        }
        offset += segment.len() + 1;
    }
    None
}

/// Returns the byte offset of the first Markdown heading line.
fn next_heading_offset(content: &str) -> Option<usize> {
    let mut offset = 0;
    for segment in content.split('\n') {
        if is_heading_line(segment) {
            return Some(offset);
        }
        offset += segment.len() + 1;
    }
    None
}

fn is_heading_line(line: &str) -> bool {
    let hashes = line.bytes().take_while(|&b| b == b'#').count();
    (1..=6).contains(&hashes) && line[hashes..].starts_with(char::is_whitespace)
}

/// Appends a whitespace-collapsed report under a new `### Report` heading.
#[must_use]
pub(in crate::task) fn append_report(body: &str, report: &str) -> String {
    let mut out = body.trim_end().to_string();
    out.push_str(REPORT_SEPARATOR);
    out.push_str(report);
    out.push('\n');
    out
}

/// Removes the last completion report appended by [`append_report`].
#[must_use]
pub(in crate::task) fn remove_report(body: &str) -> (String, Option<String>) {
    body.rsplit_once(REPORT_SEPARATOR).map_or_else(
        || (body.to_string(), None),
        |(prompt, report)| {
            (
                prompt.trim_end().to_string(),
                Some(report.trim_end().to_string()),
            )
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: &str = "\n\n";

    fn render(raw: &str) -> String {
        super::render(&TaskPrompt::new(raw))
    }

    fn is_placeholder_prompt(raw: &str) -> bool {
        super::is_placeholder_prompt(&TaskPrompt::new(raw))
    }

    fn append_lanes(body: &str, raw: &str) -> String {
        super::append_lanes(body, &TaskPrompt::new(raw))
    }

    #[test]
    fn placeholder_prompt_detection() {
        assert!(is_placeholder_prompt(""));
        assert!(is_placeholder_prompt("TODO define this"));
        assert!(is_placeholder_prompt("definir prompt"));
        assert!(!is_placeholder_prompt("add startup toggle"));
    }

    #[test]
    fn note_body_renders_marker_first_prompt_without_body_leakage() {
        assert_eq!(
            render("/c context"),
            format!("## Goals\n{S}## Context{S}- context")
        );
    }

    #[test]
    fn note_body_wraps_a_normal_prompt() {
        assert_eq!(render("add startup toggle"), "## Goals\n");
        assert_eq!(render("a / b"), format!("## Goals{S}- b"));
    }

    #[test]
    fn note_body_renders_one_bullet_per_slash_lane() {
        assert_eq!(
            render("create engine feature to add update task / make it idempotent"),
            format!("## Goals{S}- make it idempotent")
        );
    }

    #[test]
    fn note_body_preserves_ampersands_as_text() {
        assert_eq!(render("a & b"), "## Goals\n");
    }

    #[test]
    fn note_body_keeps_placeholder_raw_so_it_stays_detectable() {
        assert_eq!(render("TODO"), "TODO");
        assert!(is_placeholder_prompt(&render("TODO")));
        assert!(is_placeholder_prompt(&render("tbd")));
        assert!(is_placeholder_prompt(&render("define prompt")));
        assert_eq!(render("tbd"), "tbd");
        assert_eq!(render("define prompt"), "define prompt");
    }

    #[test]
    fn placeholder_detection_matches_the_legacy_regex_cases() {
        for raw in [
            "",
            "   ",
            "TODO",
            "todo",
            "TODO: implement",
            "[!] TODO",
            "[!]TODO",
            "tbd",
            "TBD later",
            "define prompt",
            "definir prompt",
            "the plan is tbd",
        ] {
            assert!(is_placeholder_prompt(raw), "should be placeholder: {raw:?}");
        }
        for raw in [
            "add startup toggle",
            "todolist cleanup",
            "a / b",
            "/c context",
        ] {
            assert!(
                !is_placeholder_prompt(raw),
                "should not be placeholder: {raw:?}"
            );
        }
    }

    #[test]
    fn unicode_todo_suffix_remains_raw_without_becoming_placeholder() {
        for prompt in ["TODOé", "[!] TODOé"] {
            assert!(!is_placeholder_prompt(prompt));
            assert_eq!(render(prompt), prompt);
        }
        assert!(is_placeholder_prompt("TODO-implement"));
    }

    #[test]
    fn append_lanes_grows_an_existing_section_in_place() {
        assert_eq!(
            append_lanes("## Goals\n- do the thing\n", "also this"),
            "## Goals\n- do the thing\n- also this\n"
        );
    }

    #[test]
    fn append_lanes_creates_a_missing_section_at_the_end() {
        assert_eq!(
            append_lanes("## Goals\n- do the thing\n", "another goal /c new context"),
            "## Goals\n- do the thing\n- another goal\n\n## Context\n\n- new context\n"
        );
    }

    #[test]
    fn append_lanes_marker_first_leaves_goals_untouched() {
        assert_eq!(
            append_lanes("## Goals\n- do the thing\n", "/c context"),
            "## Goals\n- do the thing\n\n## Context\n\n- context\n"
        );
    }

    #[test]
    fn append_report_collapses_multiline_into_a_single_line_block() {
        assert_eq!(
            append_report("body\n", "line one line two"),
            "body\n\n### Report\n\nline one line two\n"
        );
    }
}
