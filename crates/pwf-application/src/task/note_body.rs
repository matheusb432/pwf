//! Applies prompt, lane, and report transforms to task note bodies.

use lazy_regex::{Regex, regex};
use prompt_lanes::{Adapter, MarkdownAdapter, parse};

const REPORT_HEADER: &str = "### Report";
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
pub(in crate::task) fn render(prompt: &str) -> String {
    match prompt_classification(prompt) {
        PromptClassification::Placeholder | PromptClassification::AuthoredVerbatimLegacy => {
            prompt.to_string()
        }
        PromptClassification::Authored => MarkdownAdapter.render(&parse(prompt)),
    }
}

/// Reports whether a prompt is empty or matches `TODO`, `[!] TODO`, `define prompt`, `definir
/// prompt`, or `tbd` case-insensitively.
#[must_use]
pub(in crate::task) fn is_placeholder_prompt(prompt: &str) -> bool {
    matches!(
        prompt_classification(prompt),
        PromptClassification::Placeholder
    )
}

fn prompt_classification(prompt: &str) -> PromptClassification {
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
        .any(prompt_starts_with_todo_word_boundary_ascii)
    {
        PromptClassification::AuthoredVerbatimLegacy
    } else {
        PromptClassification::Authored
    }
}

fn prompt_starts_with_todo_word_boundary_ascii(prompt: &str) -> bool {
    prompt.strip_prefix("todo").is_some_and(|rest| {
        !rest.starts_with(|character: char| character.is_ascii_alphanumeric() || character == '_')
    })
}

/// Splices lane bullets into existing sections and appends missing sections.
///
/// Returns [`None`] for a whitespace-only prompt.
#[must_use]
pub(in crate::task) fn append_lanes(body: &str, prompt: &str) -> Option<String> {
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return None;
    }
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
    Some(out)
}

fn append_bullets_to_section(content: &str, header: &str, bullets: &[String]) -> String {
    if bullets.is_empty() {
        return content.to_string();
    }
    let Some(header_end) = header_line_end(content, header) else {
        let mut out = content.trim_end().to_string();
        out.push_str("\n\n");
        out.push_str(header);
        for bullet in bullets {
            out.push_str("\n- ");
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
    for bullet in bullets {
        out.push_str("\n- ");
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

/// Appends a free-form Markdown report verbatim under one `### Report` heading.
///
/// Returns [`None`] for a whitespace-only report.
#[must_use]
pub(in crate::task) fn append_report_block(body: &str, report: &str) -> Option<String> {
    let report = report.trim();
    if report.is_empty() {
        return None;
    }
    let mut out = body.trim_end().to_string();
    if !has_report_header(&out) {
        out.push_str("\n\n");
        out.push_str(REPORT_HEADER);
    }
    out.push_str("\n\n");
    out.push_str(report);
    out.push('\n');
    Some(out)
}

fn has_report_header(content: &str) -> bool {
    content.lines().any(|line| {
        line.strip_prefix(REPORT_HEADER)
            .is_some_and(|rest| rest.trim().is_empty())
    })
}

/// Appends a whitespace-collapsed report under a new `### Report` heading.
///
/// Returns [`None`] for a blank report.
#[must_use]
pub(in crate::task) fn append_report(body: &str, report: &str) -> Option<String> {
    let report = normalized_report(report)?;
    let mut out = body.trim_end().to_string();
    out.push_str("\n\n");
    out.push_str(REPORT_HEADER);
    out.push_str("\n\n");
    out.push_str(&report);
    out.push('\n');
    Some(out)
}

fn normalized_report(report: &str) -> Option<String> {
    let mut parts = Vec::new();
    for line in report.lines() {
        let line = line.trim();
        if !line.is_empty() {
            parts.push(line);
        }
    }
    (!parts.is_empty()).then(|| parts.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: &str = "\n\n";

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
            append_lanes("## Goals\n- do the thing\n", "also this").unwrap(),
            "## Goals\n- do the thing\n- also this\n"
        );
    }

    #[test]
    fn append_lanes_creates_a_missing_section_at_the_end() {
        assert_eq!(
            append_lanes("## Goals\n- do the thing\n", "another goal /c new context").unwrap(),
            "## Goals\n- do the thing\n- another goal\n\n## Context\n- new context\n"
        );
    }

    #[test]
    fn append_lanes_marker_first_leaves_goals_untouched() {
        assert_eq!(
            append_lanes("## Goals\n- do the thing\n", "/c context").unwrap(),
            "## Goals\n- do the thing\n\n## Context\n- context\n"
        );
    }

    #[test]
    fn append_lanes_rejects_whitespace_only() {
        assert_eq!(append_lanes("## Goals\n- x\n", "   \n\t"), None);
    }

    #[test]
    fn append_report_block_appends_verbatim_and_reuses_an_existing_header() {
        let body = "## Goals\n\n- ship it\n";
        let report = "## Outcome\n\nShipped it.\n\n## Follow-ups\n\n- write the release notes";
        assert_eq!(
            append_report_block(body, report).unwrap(),
            "## Goals\n\n- ship it\n\n### Report\n\n## Outcome\n\nShipped it.\n\n## Follow-ups\n\n- write the release notes\n"
        );
        let once = append_report_block(body, "first").unwrap();
        let twice = append_report_block(&once, "second").unwrap();
        assert_eq!(twice.matches("### Report").count(), 1, "{twice}");
    }

    #[test]
    fn append_report_block_rejects_whitespace_only() {
        assert_eq!(append_report_block("body\n", "   \n\t"), None);
    }

    #[test]
    fn append_report_collapses_multiline_into_a_single_line_block() {
        assert_eq!(
            append_report("body\n", "line one\n\nline two").unwrap(),
            "body\n\n### Report\n\nline one line two\n"
        );
        assert_eq!(append_report("body\n", "  \n\t"), None);
    }
}
