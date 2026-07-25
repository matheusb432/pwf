use std::{fmt::Write, sync::LazyLock};

use pwf_domain::pending_work::{Tags, WorkItemStatus};
use regex::Regex;

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

const UTF8_BOM: char = '\u{feff}';

#[derive(Clone, Copy)]
pub(super) struct NewWorkItemFields<'a> {
    pub id: &'a str,
    pub title: &'a str,
    pub project: &'a str,
    pub prompt: &'a str,
    pub created: &'a str,
    pub prereq: Option<&'a str>,
    pub effort: Option<u8>,
    pub tags: Option<&'a Tags>,
}

pub(super) fn new_work_item_content(fields: NewWorkItemFields<'_>) -> String {
    let mut out = String::new();
    out.push_str("---\n");
    let _ = writeln!(out, "id: {}", fields.id);
    let _ = writeln!(out, "status: {}", WorkItemStatus::Active);
    let _ = writeln!(out, "title: {}", fields.title);
    let _ = writeln!(out, "project: {}", fields.project);
    let _ = writeln!(out, "created: {}", fields.created);
    if let Some(prereq) = fields.prereq {
        let _ = writeln!(out, "prereq: \"{prereq}\"");
    }
    if let Some(effort) = fields.effort {
        let _ = writeln!(out, "effort: {effort}");
    }
    if let Some(tags) = fields.tags {
        let _ = writeln!(out, "tags: {}", tags_frontmatter_value(tags));
    }
    out.push_str("---\n\n");
    out.push_str(fields.prompt.trim_end());
    out.push('\n');
    out
}

pub(super) fn set_status_text(content: &str, status: WorkItemStatus, completed: &str) -> String {
    let status = status.as_frontmatter_str();
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
        value.map(|value| format!("prereq: \"{value}\"")),
    )
}

pub(super) fn set_completed_text(content: &str, value: Option<&str>) -> String {
    set_frontmatter_line(
        content,
        &COMPLETED_LINE_RE,
        &COMPLETED_LINE_NL_RE,
        value.map(|value| format!("completed: {value}")),
    )
}

pub(super) fn set_commits_text(content: &str, value: Option<&str>) -> String {
    set_frontmatter_line(
        content,
        &COMMITS_LINE_RE,
        &COMMITS_LINE_NL_RE,
        value.map(|value| format!("commits: \"{value}\"")),
    )
}

pub(super) fn set_effort_text(content: &str, value: Option<u8>) -> String {
    set_frontmatter_line(
        content,
        &EFFORT_LINE_RE,
        &EFFORT_LINE_NL_RE,
        value.map(|value| format!("effort: {value}")),
    )
}

pub(super) fn set_tags_text(content: &str, value: Option<&Tags>) -> String {
    set_frontmatter_line(
        content,
        &TAGS_LINE_RE,
        &TAGS_LINE_NL_RE,
        value.map(|tags| format!("tags: {}", tags_frontmatter_value(tags))),
    )
}

fn tags_frontmatter_value(tags: &Tags) -> String {
    format!(
        "[{}]",
        tags.iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    )
}
