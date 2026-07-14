use std::{fmt::Write, sync::LazyLock};

use prompt_lanes::{Adapter, MarkdownAdapter};
use pwf_domain::pending_work::TaskTitle;
use regex::Regex;

static PLACEHOLDER_PROMPT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(^\s*\[!\]\s*TODO\b|^\s*TODO\b|definir prompt|define prompt|tbd)").unwrap()
});
static TITLE_LINE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^title:.*$").unwrap());
static FRONTMATTER_FENCE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^---[ \t]*\r?$").unwrap());
static HEADING_LINE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^#{1,6}\s.*$").unwrap());
static REPORT_HEADER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^### Report\s*$").unwrap());

const MAX_TITLE_CHARS: usize = 80;
const REPORT_HEADER: &str = "### Report";
const LANE_SECTION_HEADERS: [&str; 4] =
    ["## Goals", "## Context", "## Constraints", "## Done When"];

pub(super) fn normalize_title(title: &str) -> String {
    title.to_lowercase()
}

pub(super) fn inferred_title(prompt: &str) -> String {
    let title = normalize_title(&prompt_lanes::parse(prompt).capped_title(MAX_TITLE_CHARS));
    if title.is_empty() {
        TaskTitle::default().to_string()
    } else {
        title
    }
}

pub(super) fn note_body(prompt: &str) -> String {
    if is_placeholder_prompt(prompt) {
        prompt.to_string()
    } else {
        MarkdownAdapter.render(&prompt_lanes::parse(prompt))
    }
}

fn is_placeholder_prompt(prompt: &str) -> bool {
    if prompt.trim().is_empty() {
        return true;
    }
    PLACEHOLDER_PROMPT_RE.is_match(prompt)
}

pub(super) fn replace_title(content: &str, title: &str) -> String {
    TITLE_LINE_RE
        .replace(content, format!("title: {title}").as_str())
        .into_owned()
}

pub(super) fn replace_body(content: &str, body: &str) -> String {
    let mut fences = FRONTMATTER_FENCE_RE.find_iter(content);
    match (fences.next(), fences.next()) {
        (Some(_), Some(close)) => {
            format!("{}\n\n{}\n", &content[..close.end()], body.trim_end())
        }
        _ => format!("{}\n", body.trim_end()),
    }
}

pub(super) fn append_lanes_text(content: &str, prompt: &str) -> Option<String> {
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return None;
    }
    let parsed = prompt_lanes::parse(prompt);
    let sections: [&[String]; 4] = [
        &parsed.goals,
        &parsed.context,
        &parsed.constraints,
        &parsed.done_when,
    ];
    let mut out = content.to_string();
    for (header, bullets) in LANE_SECTION_HEADERS.iter().zip(sections) {
        out = append_bullets_to_section(&out, header, bullets);
    }
    Some(out)
}

fn append_bullets_to_section(content: &str, header: &str, bullets: &[String]) -> String {
    if bullets.is_empty() {
        return content.to_string();
    }
    let header_re = Regex::new(&format!(r"(?m)^{}\s*$", regex::escape(header))).unwrap();
    let Some(header_match) = header_re.find(content) else {
        let mut out = content.trim_end().to_string();
        out.push_str("\n\n");
        out.push_str(header);
        for bullet in bullets {
            let _ = write!(out, "\n- {bullet}");
        }
        out.push('\n');
        return out;
    };
    let rest = &content[header_match.end()..];
    let section_end =
        header_match.end() + HEADING_LINE_RE.find(rest).map_or(rest.len(), |m| m.start());
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

pub(super) fn append_report_block_text(content: &str, report: &str) -> Option<String> {
    let report = report.trim();
    if report.is_empty() {
        return None;
    }
    let mut out = content.trim_end().to_string();
    if !REPORT_HEADER_RE.is_match(&out) {
        out.push_str("\n\n");
        out.push_str(REPORT_HEADER);
    }
    out.push_str("\n\n");
    out.push_str(report);
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

pub(super) fn append_report_text(content: &str, report: &str) -> Option<String> {
    let report = normalized_report(report)?;
    let mut out = content.trim_end().to_string();
    out.push_str("\n\n");
    out.push_str(REPORT_HEADER);
    out.push_str("\n\n");
    out.push_str(&report);
    out.push('\n');
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_first_title_inference_uses_domain_default_without_body_leakage() {
        assert_eq!(inferred_title("/c context"), "n/a");
        assert_eq!(note_body("/c context"), "## Goals\n\n## Context\n- context");
    }
}
