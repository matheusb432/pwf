//! Pure note-body text transforms: title inference, prompt-to-body rendering,
//! and the lane/report body splices. These are representation-agnostic string
//! functions — they never touch frontmatter fences (that stays in the vault
//! adapter's `replace_body`/`replace_title`) — so they live in the domain and
//! are shared by the application update handler and the infra add/close paths.
//!
//! Deliberately regex-free: the only external dependency is the zero-dependency
//! `prompt-lanes` lane parser; every header match here is exact line matching.

use std::fmt::Write;

use prompt_lanes::{Adapter, MarkdownAdapter, parse};

use crate::pending_work::TaskTitle;

/// Upper bound (in `char`s) on an auto-inferred title. Without it, a long prompt
/// with no explicit marker becomes unreadable in the index/list surfaces.
const MAX_TITLE_CHARS: usize = 80;
const REPORT_HEADER: &str = "### Report";
const LANE_SECTION_HEADERS: [&str; 4] =
    ["## Goals", "## Context", "## Constraints", "## Done When"];

/// Lowercases a title (the canonical pending-work title form).
#[must_use]
pub fn normalize_title(title: &str) -> String {
    title.to_lowercase()
}

/// The inferred title for a pending-work prompt — the capped, lowercased lead
/// title, falling back to the [`TaskTitle`] default when the prompt is
/// marker-first (no authored title).
#[must_use]
pub fn inferred_title(prompt: &str) -> String {
    let title = normalize_title(&parse(prompt).capped_title(MAX_TITLE_CHARS));
    if title.is_empty() {
        TaskTitle::default().to_string()
    } else {
        title
    }
}

/// Renders the pending-work note body for a prompt: a placeholder prompt is kept
/// verbatim so it stays detectable, otherwise the lane parser output is rendered
/// through the Markdown adapter.
#[must_use]
pub fn note_body(prompt: &str) -> String {
    if is_placeholder_prompt(prompt) {
        prompt.to_string()
    } else {
        MarkdownAdapter.render(&parse(prompt))
    }
}

/// Whether a prompt is a placeholder (empty, a `TODO`/`[!] TODO` marker, or a
/// `define prompt`/`definir prompt`/`tbd` sentinel).
#[must_use]
pub fn is_placeholder_prompt(prompt: &str) -> bool {
    if prompt.trim().is_empty() {
        return true;
    }
    let lower = prompt.to_lowercase();
    if lower.contains("definir prompt") || lower.contains("define prompt") || lower.contains("tbd")
    {
        return true;
    }
    let trimmed = lower.trim_start();
    let after_marker = trimmed.strip_prefix("[!]").map(str::trim_start);
    [Some(trimmed), after_marker]
        .into_iter()
        .flatten()
        .any(starts_with_todo_word)
}

fn starts_with_todo_word(text: &str) -> bool {
    text.strip_prefix("todo")
        .is_some_and(|rest| !rest.starts_with(is_word_char))
}

fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Splices lane-syntax bullets (Goals/Context/Constraints/Done When) into a
/// note body. An existing section grows in place (before the next heading), a
/// missing one is created at the end. A whitespace-only prompt yields `None`.
#[must_use]
pub fn append_lanes(body: &str, prompt: &str) -> Option<String> {
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return None;
    }
    let parsed = parse(prompt);
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
            let _ = write!(out, "\n- {bullet}");
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
        let _ = write!(out, "\n- {bullet}");
    }
    if after.is_empty() {
        out.push('\n');
    } else {
        out.push_str("\n\n");
        out.push_str(after);
    }
    out
}

/// Byte offset just past a header line (`## Goals`, optionally with trailing
/// whitespace), or `None` when the header is absent — the regex-free equivalent
/// of `^{header}\s*$`.
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

/// Byte offset of the first markdown heading line (`^#{1,6}\s`), regex-free.
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

/// Appends a free-form, multi-line Markdown report verbatim under a `### Report`
/// H3 (created if absent). A whitespace-only report yields `None`.
#[must_use]
pub fn append_report_block(body: &str, report: &str) -> Option<String> {
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

/// Appends a single-line (whitespace-collapsed) report under a fresh `### Report`
/// H3. A blank-only report yields `None`.
#[must_use]
pub fn append_report(body: &str, report: &str) -> Option<String> {
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

    #[test]
    fn marker_first_title_inference_uses_domain_default_without_body_leakage() {
        assert_eq!(inferred_title("/c context"), "n/a");
        assert_eq!(note_body("/c context"), "## Goals\n\n## Context\n- context");
    }

    #[test]
    fn note_body_wraps_a_normal_prompt_and_keeps_placeholders_raw() {
        assert_eq!(
            note_body("add startup toggle"),
            "## Goals\n- add startup toggle"
        );
        assert_eq!(note_body("a / b"), "## Goals\n- a\n- b");
        assert_eq!(note_body("TODO"), "TODO");
        assert_eq!(note_body("tbd"), "tbd");
        assert_eq!(note_body("define prompt"), "define prompt");
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
    fn normalize_title_lowercases() {
        assert_eq!(normalize_title("HUMAN: Do AZ-104"), "human: do az-104");
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
        let report = "## Outcome\n\nShipped it.\n\n## Follow-ups\n\n- write the FSD card";
        assert_eq!(
            append_report_block(body, report).unwrap(),
            "## Goals\n\n- ship it\n\n### Report\n\n## Outcome\n\nShipped it.\n\n## Follow-ups\n\n- write the FSD card\n"
        );
        // A second block reuses the existing `### Report` header (no duplicate).
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
