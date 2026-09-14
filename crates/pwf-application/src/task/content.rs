//! Applies body, section, and report transforms to task file bodies.

use lazy_regex::{Regex, regex};
use pwf_marker_sections::ParsedMarkerSections;
use pwf_models::task::TaskBody;
use pwf_wire::task::{TaskMarkerSection, TaskMarkerSectionEdits};

use super::marker_sections::TaskMarkerSections;

const REPORT_SEPARATOR: &str = "\n\n### Report\n\n";
const TASK_MARKER_SECTIONS: [TaskMarkerSection; 4] = [
    TaskMarkerSection::Goal,
    TaskMarkerSection::Context,
    TaskMarkerSection::Constraint,
    TaskMarkerSection::DoneWhen,
];

fn placeholder_body_regex() -> &'static Regex {
    regex!(r"(?i)(^\s*\[!\]\s*TODO\b|^\s*TODO\b|definir prompt|define prompt|tbd)")
}

enum BodyClassification {
    Placeholder,
    AuthoredVerbatimLegacy,
    Authored,
}

#[must_use]
pub(in crate::task) fn render(body: &str, sections: &TaskMarkerSections) -> String {
    match body_classification(body) {
        BodyClassification::Placeholder | BodyClassification::AuthoredVerbatimLegacy => {
            body.to_string()
        }
        BodyClassification::Authored => sections.render(&sections.parse(body.as_ref())),
    }
}

#[must_use]
pub(in crate::task) fn render_marker_sections(
    task_sections: &pwf_wire::task::TaskMarkerSections,
    configuration: &TaskMarkerSections,
) -> String {
    configuration.render(&ParsedMarkerSections::new(
        String::new(),
        [
            task_sections.goals().to_vec(),
            task_sections.context().to_vec(),
            task_sections.constraints().to_vec(),
            task_sections.done_when().to_vec(),
        ],
    ))
}

/// Reports whether a body is empty or matches `TODO`, `[!] TODO`, the legacy `define prompt` or
/// `definir prompt` phrases, or `tbd` case-insensitively.
#[must_use]
pub(in crate::task) fn is_placeholder_body(body: &TaskBody) -> bool {
    matches!(
        body_classification(body.as_ref()),
        BodyClassification::Placeholder
    )
}

fn body_classification(body: &str) -> BodyClassification {
    if body.trim().is_empty() || placeholder_body_regex().is_match(body) {
        return BodyClassification::Placeholder;
    }

    // Rendering uses ASCII boundaries, but diagnostics use Unicode boundaries.
    let body_lowercase = body.to_lowercase();
    let body_trimmed = body_lowercase.trim_start();
    let body_after_marker = body_trimmed.strip_prefix("[!]").map(str::trim_start);
    if [Some(body_trimmed), body_after_marker]
        .into_iter()
        .flatten()
        .any(starts_with_todo_word_boundary_ascii)
    {
        BodyClassification::AuthoredVerbatimLegacy
    } else {
        BodyClassification::Authored
    }
}

fn starts_with_todo_word_boundary_ascii(text: &str) -> bool {
    text.strip_prefix("todo").is_some_and(|rest| {
        !rest.starts_with(|character: char| character.is_ascii_alphanumeric() || character == '_')
    })
}

/// Splices section bullets into existing sections and appends missing sections.
#[must_use]
pub(in crate::task) fn append_marker_sections(
    body: &str,
    shorthand: &str,
    configuration: &TaskMarkerSections,
) -> String {
    let shorthand = shorthand.trim();
    debug_assert!(!shorthand.is_empty(), "task append bodies are validated");
    let (title, mut sections) = configuration.parse(shorthand).into_parts();
    if !title.is_empty() {
        sections[0].insert(0, title);
    }
    let mut out = body.to_string();
    for (section, bullets) in TASK_MARKER_SECTIONS.into_iter().zip(&sections) {
        out = append_bullets_to_section(&out, configuration.header(section), bullets);
    }
    out
}

#[derive(Debug, Clone, thiserror::Error)]
pub(in crate::task) enum EditMarkerSectionsError {
    #[error("task body contains more than one `{header}` section")]
    DuplicateSection { header: String },
}

pub(in crate::task) fn edit_marker_sections(
    body: &str,
    edits: &TaskMarkerSectionEdits,
    configuration: &TaskMarkerSections,
) -> Result<Option<String>, EditMarkerSectionsError> {
    reject_duplicate_marker_sections(body, configuration)?;
    let mut edited = body.to_string();

    for (section, remove) in [
        (
            TaskMarkerSection::Goal,
            edits.removals().contains(&TaskMarkerSection::Goal),
        ),
        (
            TaskMarkerSection::Context,
            edits.removals().contains(&TaskMarkerSection::Context),
        ),
        (
            TaskMarkerSection::Constraint,
            edits.removals().contains(&TaskMarkerSection::Constraint),
        ),
        (
            TaskMarkerSection::DoneWhen,
            edits.removals().contains(&TaskMarkerSection::DoneWhen),
        ),
    ] {
        if remove {
            edited = clear_marker_section(&edited, section, configuration);
        }
    }

    for (section, values) in [
        (TaskMarkerSection::Goal, edits.additions().goals()),
        (TaskMarkerSection::Context, edits.additions().context()),
        (
            TaskMarkerSection::Constraint,
            edits.additions().constraints(),
        ),
        (TaskMarkerSection::DoneWhen, edits.additions().done_when()),
    ] {
        if !values.is_empty() {
            edited = add_marker_section_values(&edited, section, values, configuration);
        }
    }

    Ok((edited != body).then_some(edited))
}

fn reject_duplicate_marker_sections(
    body: &str,
    configuration: &TaskMarkerSections,
) -> Result<(), EditMarkerSectionsError> {
    for section in TASK_MARKER_SECTIONS {
        let header = configuration.header(section);
        if header_bounds(body, header).len() > 1 {
            return Err(EditMarkerSectionsError::DuplicateSection {
                header: markdown_header(header),
            });
        }
    }
    Ok(())
}

fn clear_marker_section(
    body: &str,
    section: TaskMarkerSection,
    configuration: &TaskMarkerSections,
) -> String {
    let header = configuration.header(section);
    let Some((header_start, header_end)) = header_bounds(body, header).into_iter().next() else {
        return if section == TaskMarkerSection::Goal {
            insert_marker_section(body, section, &[], configuration)
        } else {
            body.to_string()
        };
    };
    let section_end = section_end(body, header_end);
    let replacement = (section == TaskMarkerSection::Goal).then(|| markdown_header(header));
    replace_region(body, header_start, section_end, replacement.as_deref())
}

fn add_marker_section_values(
    body: &str,
    section: TaskMarkerSection,
    values: &[String],
    configuration: &TaskMarkerSections,
) -> String {
    let header = configuration.header(section);
    if header_bounds(body, header).is_empty() {
        insert_marker_section(body, section, values, configuration)
    } else {
        append_bullets_to_section(body, header, values)
    }
}

fn insert_marker_section(
    body: &str,
    section: TaskMarkerSection,
    values: &[String],
    configuration: &TaskMarkerSections,
) -> String {
    let insertion_offset = TASK_MARKER_SECTIONS
        .into_iter()
        .filter(|candidate| marker_section_index(*candidate) > marker_section_index(section))
        .flat_map(|candidate| header_bounds(body, configuration.header(candidate)))
        .map(|(start, _)| start)
        .min()
        .unwrap_or(body.len());
    let mut inserted = markdown_header(configuration.header(section));
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
        if is_marker_section_header_line(segment, header) {
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
        if is_marker_section_header_line(segment, header) {
            return Some(offset + segment.len());
        }
        offset += segment.len() + 1;
    }
    None
}

fn is_marker_section_header_line(line: &str, header: &str) -> bool {
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

const fn marker_section_index(section: TaskMarkerSection) -> usize {
    match section {
        TaskMarkerSection::Goal => 0,
        TaskMarkerSection::Context => 1,
        TaskMarkerSection::Constraint => 2,
        TaskMarkerSection::DoneWhen => 3,
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
        |(body, report)| {
            (
                body.trim_end().to_string(),
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
        super::render(raw, &TaskMarkerSections::default_fixture())
    }

    fn is_placeholder_body(raw: &str) -> bool {
        super::is_placeholder_body(&TaskBody::new(raw))
    }

    fn append_marker_sections(body: &str, raw: &str) -> String {
        super::append_marker_sections(body, raw, &TaskMarkerSections::default_fixture())
    }

    #[test]
    fn placeholder_body_detection() {
        assert!(is_placeholder_body(""));
        assert!(is_placeholder_body("TODO define this"));
        assert!(is_placeholder_body("definir prompt"));
        assert!(!is_placeholder_body("add startup toggle"));
    }

    #[test]
    fn content_renders_marker_first_body_without_body_leakage() {
        assert_eq!(
            render("/c context"),
            format!("## Goals\n{S}## Context{S}- context")
        );
    }

    #[test]
    fn content_wraps_a_normal_body() {
        assert_eq!(render("add startup toggle"), "## Goals\n");
        assert_eq!(render("a / b"), format!("## Goals{S}- b"));
    }

    #[test]
    fn content_renders_one_bullet_per_slash_section() {
        assert_eq!(
            render("create engine feature to add update task / make it idempotent"),
            format!("## Goals{S}- make it idempotent")
        );
    }

    #[test]
    fn content_preserves_ampersands_as_text() {
        assert_eq!(render("a & b"), "## Goals\n");
    }

    #[test]
    fn content_keeps_placeholder_raw_so_it_stays_detectable() {
        assert_eq!(render("TODO"), "TODO");
        assert!(is_placeholder_body(&render("TODO")));
        assert!(is_placeholder_body(&render("tbd")));
        assert!(is_placeholder_body(&render("define prompt")));
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
            assert!(is_placeholder_body(raw), "should be placeholder: {raw:?}");
        }
        for raw in [
            "add startup toggle",
            "todolist cleanup",
            "a / b",
            "/c context",
        ] {
            assert!(
                !is_placeholder_body(raw),
                "should not be placeholder: {raw:?}"
            );
        }
    }

    #[test]
    fn unicode_todo_suffix_remains_raw_without_becoming_placeholder() {
        for body in ["TODOé", "[!] TODOé"] {
            assert!(!is_placeholder_body(body));
            assert_eq!(render(body), body);
        }
        assert!(is_placeholder_body("TODO-implement"));
    }

    #[test]
    fn append_marker_sections_grows_an_existing_section_in_place() {
        assert_eq!(
            append_marker_sections("## Goals\n- do the thing\n", "also this"),
            "## Goals\n- do the thing\n- also this\n"
        );
    }

    #[test]
    fn append_marker_sections_creates_a_missing_section_at_the_end() {
        assert_eq!(
            append_marker_sections("## Goals\n- do the thing\n", "another goal /c new context"),
            "## Goals\n- do the thing\n- another goal\n\n## Context\n\n- new context\n"
        );
    }

    #[test]
    fn append_marker_sections_marker_first_leaves_goals_untouched() {
        assert_eq!(
            append_marker_sections("## Goals\n- do the thing\n", "/c context"),
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
