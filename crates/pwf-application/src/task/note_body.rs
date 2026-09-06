//! Applies prompt, lane, and report transforms to task note bodies.

use lazy_regex::{Regex, regex};
use marker_sections::ParsedPrompt;
use pwf_models::task::TaskPrompt;
use pwf_wire::task::{TaskLane, TaskLaneEdits, TaskLanes};

use super::lane_configuration::TaskPromptLanes;

const REPORT_SEPARATOR: &str = "\n\n### Report\n\n";
const TASK_LANES: [TaskLane; 4] = [
    TaskLane::Goal,
    TaskLane::Context,
    TaskLane::Constraint,
    TaskLane::DoneWhen,
];

fn placeholder_prompt_regex() -> &'static Regex {
    regex!(r"(?i)(^\s*\[!\]\s*TODO\b|^\s*TODO\b|definir prompt|define prompt|tbd)")
}

enum PromptClassification {
    Placeholder,
    AuthoredVerbatimLegacy,
    Authored,
}

#[must_use]
pub(in crate::task) fn render(prompt: &TaskPrompt, lanes: &TaskPromptLanes) -> String {
    match prompt_classification(prompt) {
        PromptClassification::Placeholder | PromptClassification::AuthoredVerbatimLegacy => {
            prompt.to_string()
        }
        PromptClassification::Authored => lanes.render(&lanes.parse(prompt.as_ref())),
    }
}

#[must_use]
pub(in crate::task) fn render_lanes(
    task_lanes: &TaskLanes,
    configuration: &TaskPromptLanes,
) -> String {
    configuration.render(&ParsedPrompt::new(
        String::new(),
        [
            task_lanes.goals().to_vec(),
            task_lanes.context().to_vec(),
            task_lanes.constraints().to_vec(),
            task_lanes.done_when().to_vec(),
        ],
    ))
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
#[must_use]
pub(in crate::task) fn append_lanes(
    body: &str,
    prompt: &TaskPrompt,
    configuration: &TaskPromptLanes,
) -> String {
    let prompt = prompt.as_ref().trim();
    debug_assert!(!prompt.is_empty(), "task append prompts are validated");
    let (title, mut sections) = configuration.parse(prompt).into_parts();
    if !title.is_empty() {
        sections[0].insert(0, title);
    }
    let mut out = body.to_string();
    for (lane, bullets) in TASK_LANES.into_iter().zip(&sections) {
        out = append_bullets_to_section(&out, configuration.header(lane), bullets);
    }
    out
}

#[derive(Debug, Clone, thiserror::Error)]
pub(in crate::task) enum EditLanesError {
    #[error("task body contains more than one `{header}` section")]
    DuplicateSection { header: String },
}

pub(in crate::task) fn edit_lanes(
    body: &str,
    edits: &TaskLaneEdits,
    configuration: &TaskPromptLanes,
) -> Result<Option<String>, EditLanesError> {
    reject_duplicate_lane_sections(body, configuration)?;
    let mut edited = body.to_string();

    for (lane, remove) in [
        (TaskLane::Goal, edits.removals().contains(&TaskLane::Goal)),
        (
            TaskLane::Context,
            edits.removals().contains(&TaskLane::Context),
        ),
        (
            TaskLane::Constraint,
            edits.removals().contains(&TaskLane::Constraint),
        ),
        (
            TaskLane::DoneWhen,
            edits.removals().contains(&TaskLane::DoneWhen),
        ),
    ] {
        if remove {
            edited = clear_lane_section(&edited, lane, configuration);
        }
    }

    for (lane, values) in [
        (TaskLane::Goal, edits.additions().goals()),
        (TaskLane::Context, edits.additions().context()),
        (TaskLane::Constraint, edits.additions().constraints()),
        (TaskLane::DoneWhen, edits.additions().done_when()),
    ] {
        if !values.is_empty() {
            edited = add_lane_values(&edited, lane, values, configuration);
        }
    }

    Ok((edited != body).then_some(edited))
}

fn reject_duplicate_lane_sections(
    body: &str,
    configuration: &TaskPromptLanes,
) -> Result<(), EditLanesError> {
    for lane in TASK_LANES {
        let header = configuration.header(lane);
        if header_bounds(body, header).len() > 1 {
            return Err(EditLanesError::DuplicateSection {
                header: markdown_header(header),
            });
        }
    }
    Ok(())
}

fn clear_lane_section(body: &str, lane: TaskLane, configuration: &TaskPromptLanes) -> String {
    let header = configuration.header(lane);
    let Some((header_start, header_end)) = header_bounds(body, header).into_iter().next() else {
        return if lane == TaskLane::Goal {
            insert_lane_section(body, lane, &[], configuration)
        } else {
            body.to_string()
        };
    };
    let section_end = section_end(body, header_end);
    let replacement = (lane == TaskLane::Goal).then(|| markdown_header(header));
    replace_region(body, header_start, section_end, replacement.as_deref())
}

fn add_lane_values(
    body: &str,
    lane: TaskLane,
    values: &[String],
    configuration: &TaskPromptLanes,
) -> String {
    let header = configuration.header(lane);
    if header_bounds(body, header).is_empty() {
        insert_lane_section(body, lane, values, configuration)
    } else {
        append_bullets_to_section(body, header, values)
    }
}

fn insert_lane_section(
    body: &str,
    lane: TaskLane,
    values: &[String],
    configuration: &TaskPromptLanes,
) -> String {
    let insertion_offset = TASK_LANES
        .into_iter()
        .filter(|candidate| lane_index(*candidate) > lane_index(lane))
        .flat_map(|candidate| header_bounds(body, configuration.header(candidate)))
        .map(|(start, _)| start)
        .min()
        .unwrap_or(body.len());
    let mut inserted = markdown_header(configuration.header(lane));
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
        if is_lane_header_line(segment, header) {
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
        out.push_str("## ");
        out.push_str(header);
        append_bullets(&mut out, bullets, "\n\n- ");
        out.push('\n');
        return out;
    };
    let rest = &content[header_end..];
    let section_end = header_end + next_heading_offset(rest).unwrap_or(rest.len());
    let before = content[..section_end].trim_end_matches('\n');
    let after = content[section_end..].trim_start_matches('\n');
    let mut out = before.to_string();
    let section_is_empty = content[header_end..section_end].trim().is_empty();
    let first_separator = if section_is_empty { "\n\n- " } else { "\n- " };
    append_bullets(&mut out, bullets, first_separator);
    if after.is_empty() {
        out.push('\n');
    } else {
        out.push_str("\n\n");
        out.push_str(after);
    }
    out
}

fn append_bullets(out: &mut String, bullets: &[String], first_separator: &'static str) {
    let mut separator = first_separator;
    for bullet in bullets {
        out.push_str(separator);
        out.push_str(bullet);
        separator = "\n- ";
    }
}

/// Returns the byte offset after an exact header line, allowing trailing whitespace.
fn header_line_end(content: &str, header: &str) -> Option<usize> {
    let mut offset = 0;
    for segment in content.split('\n') {
        if is_lane_header_line(segment, header) {
            return Some(offset + segment.len());
        }
        offset += segment.len() + 1;
    }
    None
}

fn is_lane_header_line(line: &str, header: &str) -> bool {
    line.strip_prefix("## ")
        .and_then(|line| line.strip_prefix(header))
        .is_some_and(|rest| rest.trim().is_empty())
}

fn markdown_header(header: &str) -> String {
    let mut markdown = String::with_capacity("## ".len() + header.len());
    markdown.push_str("## ");
    markdown.push_str(header);
    markdown
}

const fn lane_index(lane: TaskLane) -> usize {
    match lane {
        TaskLane::Goal => 0,
        TaskLane::Context => 1,
        TaskLane::Constraint => 2,
        TaskLane::DoneWhen => 3,
    }
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
        super::render(&TaskPrompt::new(raw), &TaskPromptLanes::default_fixture())
    }

    fn is_placeholder_prompt(raw: &str) -> bool {
        super::is_placeholder_prompt(&TaskPrompt::new(raw))
    }

    fn append_lanes(body: &str, raw: &str) -> String {
        super::append_lanes(
            body,
            &TaskPrompt::new(raw),
            &TaskPromptLanes::default_fixture(),
        )
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
