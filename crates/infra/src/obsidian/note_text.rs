use std::{fmt::Write, sync::LazyLock};

use prompt_lanes::{Adapter, MarkdownAdapter};
use pwf_domain::pending_work::Tags;
use regex::Regex;

static PLACEHOLDER_PROMPT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(^\s*\[!\]\s*TODO\b|^\s*TODO\b|definir prompt|define prompt|tbd)").unwrap()
});
static TITLE_LINE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^title:.*$").unwrap());
static STATUS_LINE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^status:.*$").unwrap());
static COMPLETED_LINE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^completed:.*$").unwrap());
static COMPLETED_LINE_NL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^completed:.*\n?").unwrap());
static CREATED_LINE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^created:.*$").unwrap());
static PREREQ_LINE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^prereq:.*$").unwrap());
static PREREQ_LINE_NL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^prereq:.*\n?").unwrap());
static COMMITS_LINE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^commits:.*$").unwrap());
static COMMITS_LINE_NL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^commits:.*\n?").unwrap());
static EFFORT_LINE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^effort:.*$").unwrap());
static EFFORT_LINE_NL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^effort:.*\n?").unwrap());
static TAGS_LINE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^tags:.*$").unwrap());
static TAGS_LINE_NL_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^tags:.*\n?").unwrap());
static FRONTMATTER_FENCE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^---[ \t]*\r?$").unwrap());
static HEADING_LINE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^#{1,6}\s.*$").unwrap());
static REPORT_HEADER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^### Report\s*$").unwrap());

const MAX_TITLE_CHARS: usize = 80;
const REPORT_HEADER: &str = "### Report";
const UTF8_BOM: char = '\u{feff}';
const LANE_SECTION_HEADERS: [&str; 4] =
    ["## Goals", "## Context", "## Constraints", "## Done When"];

#[derive(Clone, Copy)]
pub(super) struct WorkItemFields<'a> {
    pub title: &'a str,
    pub project: &'a str,
    pub prompt: &'a str,
    pub status: &'a str,
    pub created: &'a str,
    pub completed: Option<&'a str>,
    pub prereq: Option<&'a str>,
    pub effort: Option<u8>,
    pub tags: Option<&'a Tags>,
}

pub(super) fn normalize_title(title: &str) -> String {
    title.to_lowercase()
}

pub(super) fn inferred_title(prompt: &str) -> String {
    normalize_title(&prompt_lanes::parse(prompt).capped_title(MAX_TITLE_CHARS))
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

pub(super) fn work_item_content(fields: WorkItemFields<'_>) -> String {
    let mut out = String::new();
    out.push_str("---\n");
    let _ = writeln!(out, "status: {}", fields.status);
    let _ = writeln!(out, "title: {}", fields.title);
    let _ = writeln!(out, "project: {}", fields.project);
    let _ = writeln!(out, "created: {}", fields.created);
    if let Some(completed) = fields.completed {
        let _ = writeln!(out, "completed: {completed}");
    }
    if let Some(prereq) = fields.prereq {
        let _ = writeln!(out, "prereq: \"{prereq}\"");
    }
    if let Some(effort) = fields.effort {
        let _ = writeln!(out, "effort: {effort}");
    }
    if let Some(tags) = fields.tags {
        let _ = writeln!(out, "tags: {}", tags.frontmatter_value());
    }
    out.push_str("---\n\n");
    out.push_str(fields.prompt.trim_end());
    out.push('\n');
    out
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

pub(super) fn set_status_text(content: &str, status: &str, completed: &str) -> String {
    let content = STATUS_LINE_RE
        .replace(content, format!("status: {status}").as_str())
        .into_owned();
    if COMPLETED_LINE_RE.is_match(&content) {
        return COMPLETED_LINE_RE
            .replace(&content, format!("completed: {completed}").as_str())
            .into_owned();
    }
    let after = Regex::new(&format!(r"(?m)^(status: {})$", regex::escape(status))).unwrap();
    after
        .replace(
            &content,
            format!("status: {status}\ncompleted: {completed}").as_str(),
        )
        .into_owned()
}

pub(super) fn reopen_status_text(content: &str) -> String {
    let content = STATUS_LINE_RE
        .replace(content, "status: active")
        .into_owned();
    COMPLETED_LINE_NL_RE.replace(&content, "").into_owned()
}

fn set_frontmatter_line(
    content: &str,
    line_re: &Regex,
    line_nl_re: &Regex,
    line: Option<String>,
) -> String {
    let Some(bounds) = opening_frontmatter_bounds(content) else {
        return content.to_string();
    };
    let frontmatter = &content[bounds.start..bounds.end];

    let Some(line) = line else {
        let updated = line_nl_re.replace(frontmatter, "");
        return replace_frontmatter_slice(content, bounds.start, bounds.end, &updated);
    };
    if let Some(existing) = line_re.find(frontmatter) {
        let carriage_return = if existing.as_str().ends_with('\r') {
            "\r"
        } else {
            ""
        };
        let replacement = format!("{line}{carriage_return}");
        let updated = line_re.replace(frontmatter, replacement.as_str());
        return replace_frontmatter_slice(content, bounds.start, bounds.end, &updated);
    }
    for re in [&*COMPLETED_LINE_RE, &*CREATED_LINE_RE] {
        if let Some(m) = re.find(frontmatter) {
            let has_carriage_return = m.as_str().ends_with('\r');
            let line_end = bounds.start + m.end() - usize::from(has_carriage_return);
            let newline = if has_carriage_return {
                "\r\n"
            } else {
                bounds.newline
            };
            return format!(
                "{}{newline}{line}{}",
                &content[..line_end],
                &content[line_end..]
            );
        }
    }
    format!(
        "{}{line}{}{}",
        &content[..bounds.end],
        bounds.newline,
        &content[bounds.end..]
    )
}

#[derive(Clone, Copy)]
struct FrontmatterBounds {
    start: usize,
    end: usize,
    newline: &'static str,
}

fn opening_frontmatter_bounds(content: &str) -> Option<FrontmatterBounds> {
    let without_bom = content.strip_prefix(UTF8_BOM).unwrap_or(content);
    let bom_len = content.len() - without_bom.len();
    let mut fences = FRONTMATTER_FENCE_RE.find_iter(without_bom);
    let open = fences.next()?;
    let close = fences.next()?;
    if open.start() != 0 {
        return None;
    }
    let newline = if open.as_str().ends_with('\r') {
        "\r\n"
    } else {
        "\n"
    };
    Some(FrontmatterBounds {
        start: bom_len + open.end(),
        end: bom_len + close.start(),
        newline,
    })
}

fn replace_frontmatter_slice(
    content: &str,
    frontmatter_start: usize,
    frontmatter_end: usize,
    updated: &str,
) -> String {
    format!(
        "{}{}{}",
        &content[..frontmatter_start],
        updated,
        &content[frontmatter_end..]
    )
}

pub(super) fn set_prereq_text(content: &str, value: Option<&str>) -> String {
    set_frontmatter_line(
        content,
        &PREREQ_LINE_RE,
        &PREREQ_LINE_NL_RE,
        value.map(|v| format!("prereq: \"{v}\"")),
    )
}

pub(super) fn set_commits_text(content: &str, value: Option<&str>) -> String {
    set_frontmatter_line(
        content,
        &COMMITS_LINE_RE,
        &COMMITS_LINE_NL_RE,
        value.map(|v| format!("commits: \"{v}\"")),
    )
}

pub(super) fn set_effort_text(content: &str, value: Option<u8>) -> String {
    set_frontmatter_line(
        content,
        &EFFORT_LINE_RE,
        &EFFORT_LINE_NL_RE,
        value.map(|v| format!("effort: {v}")),
    )
}

pub(super) fn set_tags_text(content: &str, value: Option<&Tags>) -> String {
    set_frontmatter_line(
        content,
        &TAGS_LINE_RE,
        &TAGS_LINE_NL_RE,
        value.map(|tags| format!("tags: {}", tags.frontmatter_value())),
    )
}
