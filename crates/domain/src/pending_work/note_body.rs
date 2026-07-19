//! Pure title, prompt, lane, and report transforms for note bodies.
//!
//! Frontmatter remains a storage concern. Section headers match exact lines.

use std::{fmt::Write, sync::LazyLock};

use prompt_lanes::{Adapter, MarkdownAdapter, parse};
use regex::Regex;

use crate::pending_work::TaskTitle;

/// Maximum character count before the inferred-title ellipsis.
const MAX_TITLE_CHARS: usize = 80;
const REPORT_HEADER: &str = "### Report";
const LANE_SECTION_HEADERS: [&str; 4] =
    ["## Goals", "## Context", "## Constraints", "## Done When"];

static PLACEHOLDER_PROMPT_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(^\s*\[!\]\s*TODO\b|^\s*TODO\b|definir prompt|define prompt|tbd)")
        .expect("valid placeholder regex")
});

enum PromptClassification {
    Placeholder,
    AuthoredVerbatimLegacy,
    Authored,
}

/// YAML indicator characters that cannot open an unquoted plain scalar.
const YAML_UNSAFE_LEADING_CHARS: [char; 16] = [
    ',', '[', ']', '{', '}', '#', '&', '*', '!', '|', '>', '\'', '"', '%', '@', '`',
];

/// Lowercases a title and rewrites it into a single-line YAML-safe plain scalar.
///
/// Titles are stored as unquoted `title: <value>` frontmatter lines, so input that
/// would break or truncate the YAML mapping — `: `, trailing colons, comment-starting
/// `#`, unsafe leading indicator characters, embedded newlines — is replaced or
/// dropped. Falls back to [`TaskTitle::default`] when nothing displayable survives.
#[must_use]
pub fn normalize_title(title: &str) -> String {
    let safe = yaml_plain_scalar(&title.to_lowercase());
    if safe.is_empty() {
        TaskTitle::default().to_string()
    } else {
        safe
    }
}

/// Reports whether [`normalize_title`] changed `raw` beyond trimming and lowercasing.
#[must_use]
pub fn title_was_normalized(raw: &str) -> bool {
    normalize_title(raw) != raw.trim().to_lowercase()
}

fn yaml_plain_scalar(title: &str) -> String {
    let collapsed = title.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out = String::with_capacity(collapsed.len());
    let mut chars = collapsed.chars().peekable();
    let mut opens_comment = true;
    while let Some(c) = chars.next() {
        match c {
            ':' => {
                let mut run_length = 1;
                while chars.next_if_eq(&':').is_some() {
                    run_length += 1;
                }
                match chars.peek() {
                    None => {}                  // Trailing colons would open a nested mapping.
                    Some(' ') => out.push(';'), // `: ` opens a nested mapping.
                    Some(_) => out.extend(std::iter::repeat_n(':', run_length)),
                }
                opens_comment = false;
            }
            '#' if opens_comment => {} // `#` at start or after a space opens a comment.
            _ => {
                out.push(c);
                opens_comment = c == ' ';
            }
        }
    }
    strip_unsafe_leading_chars(out.trim_end()).to_string()
}

fn strip_unsafe_leading_chars(mut value: &str) -> &str {
    loop {
        value = value.trim_start_matches(' ');
        let mut chars = value.chars();
        let Some(first) = chars.next() else {
            return value;
        };
        // `-`, `?`, and `:` are safe unless a space (or end of value) follows them.
        let unsafe_lead = YAML_UNSAFE_LEADING_CHARS.contains(&first)
            || (matches!(first, '-' | '?' | ':')
                && chars.next().is_none_or(|second| second == ' '));
        if !unsafe_lead {
            return value;
        }
        value = &value[first.len_utf8()..];
    }
}

/// Returns the capped, normalized lead clause or [`TaskTitle::default`] for a marker-first prompt.
#[must_use]
pub fn inferred_title(prompt: &str) -> String {
    normalize_title(&parse(prompt).capped_title(MAX_TITLE_CHARS))
}

/// Renders a prompt as Markdown while preserving placeholders and verbatim-authored prompts.
#[must_use]
pub fn note_body(prompt: &str) -> String {
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
pub fn is_placeholder_prompt(prompt: &str) -> bool {
    matches!(
        prompt_classification(prompt),
        PromptClassification::Placeholder
    )
}

fn prompt_classification(prompt: &str) -> PromptClassification {
    if prompt.trim().is_empty() || PLACEHOLDER_PROMPT_REGEX.is_match(prompt) {
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

/// Appends a whitespace-collapsed report under a new `### Report` heading.
///
/// Returns [`None`] for a blank report.
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

    const MAX_RENDERED_TITLE_CHARS: usize = 81;

    #[test]
    fn title_inference_keeps_full_prompt_without_ampersand() {
        assert_eq!(
            inferred_title("Build the data filter to let users sort"),
            "build the data filter to let users sort"
        );
        assert_eq!(inferred_title("add startup toggle"), "add startup toggle");
    }

    #[test]
    fn title_inference_cuts_at_first_lane_marker_only() {
        assert_eq!(
            inferred_title("create engine feature to add update task / make it idempotent"),
            "create engine feature to add update task"
        );
        assert_eq!(inferred_title("a / b / c"), "a");
        assert_eq!(
            inferred_title("fix bug: empty prompt"),
            "fix bug; empty prompt"
        );
    }

    #[test]
    fn title_inference_collapses_whitespace_and_lowercases() {
        assert_eq!(
            inferred_title("  Refactor   Help  Command  "),
            "refactor help command"
        );
    }

    #[test]
    fn title_inference_empty_lead_uses_domain_fallback() {
        assert_eq!(inferred_title("/ only second"), "n/a");
        assert_eq!(inferred_title("/c context"), "n/a");
    }

    #[test]
    fn title_inference_caps_long_prompt_without_marker_at_word_boundary() {
        let prompt = "Continue the PowerShell to Rust port into the cfgtool CLI (scripts/cfgtool), using the shipped gaming domain as the template, porting domain-by-domain smallest first";
        let title = inferred_title(prompt);
        assert_eq!(
            title,
            "continue the powershell to rust port into the cfgtool cli (scripts/cfgtool),…"
        );
        assert!(title.chars().count() <= MAX_RENDERED_TITLE_CHARS);
        assert!(
            prompt
                .to_lowercase()
                .starts_with(title.trim_end_matches('…'))
        );
    }

    #[test]
    fn title_inference_caps_long_lead_clause_before_marker() {
        let prompt = "HUMAN: Start studying AZ-104 Section 02 - Storage. Begin with the Storage MOC, then cover Storage Accounts / Redundancy / Security";
        let title = inferred_title(prompt);
        assert_eq!(
            title,
            "human; start studying az-104 section 02 - storage. begin with the storage moc,…"
        );
        assert!(title.chars().count() <= MAX_RENDERED_TITLE_CHARS);
    }

    #[test]
    fn title_inference_short_prompt_passes_through_uncapped() {
        assert_eq!(
            inferred_title("add a startup toggle to the settings page"),
            "add a startup toggle to the settings page"
        );
    }

    #[test]
    fn title_inference_caps_single_overlong_word_on_char_boundary() {
        let word = "x".repeat(200);
        let title = inferred_title(&word);
        assert!(title.ends_with('…'));
        assert_eq!(title.chars().count(), MAX_RENDERED_TITLE_CHARS);
    }

    #[test]
    fn title_inference_is_always_bounded() {
        let cases = [
            "x".repeat(500),                           // long, no boundary
            format!("{} / tail", "word ".repeat(200)), // long lead before marker
            "/ ".repeat(300),                          // all separators
            "💥".repeat(300),                          // multi-byte, no spaces
            "a ".repeat(300),                          // many word boundaries
            String::new(),                             // empty
        ];
        for input in cases {
            let t = inferred_title(&input);
            assert!(
                t.chars().count() <= MAX_RENDERED_TITLE_CHARS,
                "input bound violated ({} chars): {t}",
                t.chars().count()
            );
        }
    }

    #[test]
    fn normalize_title_lowercases_and_leaves_safe_titles_untouched() {
        assert_eq!(normalize_title("HUMAN: Do AZ-104"), "human; do az-104");
        assert_eq!(normalize_title("already lower"), "already lower");
        assert_eq!(normalize_title("time is 3:30pm"), "time is 3:30pm");
        assert_eq!(normalize_title("fix foo::bar panic"), "fix foo::bar panic");
        assert_eq!(
            normalize_title("read https://docs.rs entry"),
            "read https://docs.rs entry"
        );
    }

    #[test]
    fn normalize_title_rewrites_mapping_breaking_colons() {
        assert_eq!(
            normalize_title("finish refactor: promote sync-git seam"),
            "finish refactor; promote sync-git seam"
        );
        assert_eq!(normalize_title("fix parser:"), "fix parser");
        assert_eq!(normalize_title("a :: b"), "a ; b");
        assert_eq!(normalize_title("fix parser :"), "fix parser");
    }

    #[test]
    fn normalize_title_drops_comment_starting_hashes() {
        assert_eq!(normalize_title("fix #123 now"), "fix 123 now");
        assert_eq!(normalize_title("# lead hash"), "lead hash");
        assert_eq!(normalize_title("close c# ticket"), "close c# ticket");
    }

    #[test]
    fn normalize_title_strips_unsafe_leading_indicator_chars() {
        assert_eq!(normalize_title("- do it"), "do it");
        assert_eq!(normalize_title("[wip] fix"), "wip] fix");
        assert_eq!(normalize_title("\"quoted start"), "quoted start");
        assert_eq!(normalize_title("? open question"), "open question");
        assert_eq!(normalize_title("-x marks the spot"), "-x marks the spot");
    }

    #[test]
    fn normalize_title_collapses_whitespace_onto_one_line() {
        assert_eq!(normalize_title("a\nb: c"), "a b; c");
        assert_eq!(normalize_title("tab\there"), "tab here");
    }

    #[test]
    fn normalize_title_falls_back_to_default_when_nothing_survives() {
        for raw in [":", ":::", "#", " : ", "- ", ""] {
            assert_eq!(normalize_title(raw), "n/a", "input: {raw:?}");
        }
    }

    #[test]
    fn title_was_normalized_ignores_case_and_outer_whitespace_changes() {
        assert!(!title_was_normalized("  Fix The THING  "));
        assert!(!title_was_normalized("time is 3:30pm"));
        assert!(title_was_normalized("finish refactor: promote seam"));
        assert!(title_was_normalized("fix #123 now"));
        assert!(title_was_normalized(""));
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
        assert_eq!(note_body("/c context"), "## Goals\n\n## Context\n- context");
    }

    #[test]
    fn note_body_wraps_a_normal_prompt() {
        assert_eq!(
            note_body("add startup toggle"),
            "## Goals\n- add startup toggle"
        );
        assert_eq!(note_body("a / b"), "## Goals\n- a\n- b");
    }

    #[test]
    fn note_body_renders_one_bullet_per_slash_lane() {
        assert_eq!(
            note_body("create engine feature to add update task / make it idempotent"),
            "## Goals\n- create engine feature to add update task\n- make it idempotent"
        );
    }

    #[test]
    fn note_body_preserves_ampersands_as_text() {
        assert_eq!(note_body("a & b"), "## Goals\n- a & b");
    }

    #[test]
    fn note_body_keeps_placeholder_raw_so_it_stays_detectable() {
        assert_eq!(note_body("TODO"), "TODO");
        assert!(is_placeholder_prompt(&note_body("TODO")));
        assert!(is_placeholder_prompt(&note_body("tbd")));
        assert!(is_placeholder_prompt(&note_body("define prompt")));
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
    fn unicode_todo_suffix_remains_raw_without_becoming_placeholder() {
        for prompt in ["TODOé", "[!] TODOé"] {
            assert!(!is_placeholder_prompt(prompt));
            assert_eq!(note_body(prompt), prompt);
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
