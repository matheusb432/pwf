use std::{fmt::Write, ops::Range};

use pwf_models::task::{EffortTier, Prerequisites, Tags, TaskId, TaskStatus, TaskTitle};

const UTF8_BOM: char = '\u{feff}';

#[derive(Clone, Copy)]
pub(super) struct NewTaskFields<'a> {
    pub id: &'a TaskId,
    pub title: &'a TaskTitle,
    pub project: &'a str,
    pub prompt: &'a str,
    pub created: &'a str,
    pub prereq: Option<&'a Prerequisites>,
    pub effort: Option<EffortTier>,
    pub tags: Option<&'a Tags>,
}

pub(super) fn new_task_content(fields: NewTaskFields<'_>) -> String {
    let mut out = String::new();
    out.push_str("---\n");
    let _ = writeln!(out, "id: {}", fields.id);
    let _ = writeln!(out, "status: {}", TaskStatus::Active);
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

pub(super) fn set_status_text(content: &str, status: TaskStatus, completed: &str) -> String {
    let status = status.as_str();
    let Some(status_range) = find_field_line(content, "status:") else {
        return content.to_string();
    };
    let status_line = format!("status: {status}");
    let content = replace_range(content, status_range.clone(), &status_line);
    if let Some(completed_range) = find_field_line(&content, "completed:") {
        return replace_range(
            &content,
            completed_range,
            &format!("completed: {completed}"),
        );
    }
    let status_end = status_range.start + status_line.len();
    format!(
        "{}\ncompleted: {completed}{}",
        &content[..status_end],
        &content[status_end..]
    )
}

pub(super) fn reopen_status_text(content: &str) -> String {
    let content = replace_field_line(content, "status:", "status: active");
    remove_field_line(&content, "completed:")
}

fn set_frontmatter_line(content: &str, field: &str, line: Option<String>) -> String {
    let Some(bounds) = opening_frontmatter_bounds(content) else {
        return content.to_string();
    };
    let frontmatter = &content[bounds.start..bounds.end];

    let Some(line) = line else {
        let updated = remove_field_line(frontmatter, field);
        return replace_frontmatter_slice(content, bounds.start, bounds.end, &updated);
    };
    if let Some(existing) = find_field_line(frontmatter, field) {
        let carriage_return = if frontmatter[existing.clone()].ends_with('\r') {
            "\r"
        } else {
            ""
        };
        let replacement = format!("{line}{carriage_return}");
        let updated = replace_range(frontmatter, existing, &replacement);
        return replace_frontmatter_slice(content, bounds.start, bounds.end, &updated);
    }
    for field in ["completed:", "created:"] {
        if let Some(existing) = find_field_line(frontmatter, field) {
            let has_carriage_return = frontmatter[existing.clone()].ends_with('\r');
            let line_end = bounds.start + existing.end - usize::from(has_carriage_return);
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
    let open = find_fence(without_bom, 0)?;
    let close = find_fence(without_bom, open.end.saturating_add(1))?;
    if open.start != 0 {
        return None;
    }
    let newline = if without_bom[open.clone()].ends_with('\r') {
        "\r\n"
    } else {
        "\n"
    };
    Some(FrontmatterBounds {
        start: bom_len + open.end,
        end: bom_len + close.start,
        newline,
    })
}

fn find_field_line(content: &str, field: &str) -> Option<Range<usize>> {
    find_line(content, 0, |line| line.starts_with(field))
}

fn find_fence(content: &str, start: usize) -> Option<Range<usize>> {
    find_line(content, start, |line| {
        let line = line.strip_suffix('\r').unwrap_or(line);
        line.strip_prefix("---").is_some_and(|suffix| {
            suffix
                .chars()
                .all(|character| matches!(character, ' ' | '\t'))
        })
    })
}

fn find_line(
    content: &str,
    mut start: usize,
    predicate: impl Fn(&str) -> bool,
) -> Option<Range<usize>> {
    while start < content.len() {
        let end = content[start..]
            .find('\n')
            .map_or(content.len(), |offset| start + offset);
        if predicate(&content[start..end]) {
            return Some(start..end);
        }
        if end == content.len() {
            return None;
        }
        start = end + 1;
    }
    None
}

fn replace_field_line(content: &str, field: &str, replacement: &str) -> String {
    find_field_line(content, field).map_or_else(
        || content.to_string(),
        |range| replace_range(content, range, replacement),
    )
}

fn remove_field_line(content: &str, field: &str) -> String {
    let Some(mut range) = find_field_line(content, field) else {
        return content.to_string();
    };
    if content.as_bytes().get(range.end) == Some(&b'\n') {
        range.end += 1;
    }
    replace_range(content, range, "")
}

fn replace_range(content: &str, range: Range<usize>, replacement: &str) -> String {
    format!(
        "{}{}{}",
        &content[..range.start],
        replacement,
        &content[range.end..]
    )
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

pub(super) fn set_prereq_text(content: &str, value: Option<&Prerequisites>) -> String {
    set_frontmatter_line(
        content,
        "prereq:",
        value.map(|value| format!("prereq: \"{value}\"")),
    )
}

pub(super) fn set_completed_text(content: &str, value: Option<&str>) -> String {
    set_frontmatter_line(
        content,
        "completed:",
        value.map(|value| format!("completed: {value}")),
    )
}

pub(super) fn set_commits_text(content: &str, value: Option<&str>) -> String {
    set_frontmatter_line(
        content,
        "commits:",
        value.map(|value| format!("commits: \"{value}\"")),
    )
}

pub(super) fn set_effort_text(content: &str, value: Option<EffortTier>) -> String {
    set_frontmatter_line(
        content,
        "effort:",
        value.map(|value| format!("effort: {value}")),
    )
}

pub(super) fn set_tags_text(content: &str, value: Option<&Tags>) -> String {
    set_frontmatter_line(
        content,
        "tags:",
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

#[cfg(test)]
mod tests {
    use pwf_models::task::TaskStatus;

    use super::set_status_text;

    #[test]
    fn close_status_inserts_completion_without_rewriting_other_bytes() {
        let source = "---\nid: PWF-0001\nstatus: active\ntitle: task\n---\n\nbody\n";

        assert_eq!(
            set_status_text(source, TaskStatus::Done, "2026-07-29"),
            "---\nid: PWF-0001\nstatus: done\ncompleted: 2026-07-29\ntitle: task\n---\n\nbody\n"
        );
    }
}
