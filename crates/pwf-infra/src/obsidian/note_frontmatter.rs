use std::{fmt::Write, ops::Range};

use gray_matter::{Matter, engine::YAML};
use pwf_application::ports::task_record::StoredBlockedBy;
use pwf_models::{
    AppDate,
    task::{BlockedBy, EffortTier, TaskId, TaskStatus, TaskTags, TaskTitle},
};
use serde::Deserialize;
use serde_json::Value;

use super::markdown_line;

const UTF8_BOM: char = '\u{feff}';

#[derive(Deserialize)]
struct BlockedByFrontmatter {
    #[serde(default)]
    blocked_by: Value,
}

#[derive(Clone, Copy)]
pub(super) struct NewTaskFields<'a> {
    pub id: &'a TaskId,
    pub title: &'a TaskTitle,
    pub project: &'a str,
    pub body: &'a str,
    pub created: &'a AppDate,
    pub blocked_by: Option<&'a BlockedBy>,
    pub effort: Option<EffortTier>,
    pub tags: Option<&'a TaskTags>,
}

pub(super) fn new_task_content(fields: NewTaskFields<'_>) -> String {
    let mut out = String::new();
    out.push_str("---\n");
    let _ = writeln!(out, "id: {}", fields.id);
    let _ = writeln!(out, "status: {}", TaskStatus::Active);
    let _ = writeln!(out, "title: {}", fields.title);
    let _ = writeln!(out, "project: {}", fields.project);
    let _ = writeln!(out, "created: {}", fields.created);
    if let Some(blocked_by) = fields.blocked_by {
        let _ = writeln!(
            out,
            "blocked_by: {}",
            blocked_by_frontmatter_value(blocked_by)
        );
    }
    if let Some(effort) = fields.effort {
        let _ = writeln!(out, "effort: {effort}");
    }
    if let Some(tags) = fields.tags {
        let _ = writeln!(out, "tags: {}", tags_frontmatter_value(tags));
    }
    out.push_str("---\n\n");
    out.push_str(fields.body.trim_end());
    out.push('\n');
    out
}

pub(super) fn set_status_text(
    content: &str,
    status: TaskStatus,
    completed: Option<&AppDate>,
) -> String {
    let status = status.as_str();
    let completed = completed.map(ToString::to_string).unwrap_or_default();
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
        let updated = remove_field_block(frontmatter, field);
        return replace_frontmatter_slice(content, bounds.start, bounds.end, &updated);
    };
    if let Some(existing) = find_field_block(frontmatter, field) {
        let Some(first_line) = find_field_line(frontmatter, field) else {
            return content.to_string();
        };
        let carriage_return = if frontmatter[first_line].ends_with('\r') {
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
    let close = find_fence(without_bom, open.end)?;
    if open.start != 0 {
        return None;
    }
    let newline = open.newline;
    Some(FrontmatterBounds {
        start: bom_len + open.content_end,
        end: bom_len + close.start,
        newline,
    })
}

fn find_field_line(content: &str, field: &str) -> Option<Range<usize>> {
    markdown_line::find(content, 0, |line| line.starts_with(field))
        .map(|line| line.start..line.content_end)
}

fn find_field_block(content: &str, field: &str) -> Option<Range<usize>> {
    let first = markdown_line::find(content, 0, |line| line.starts_with(field))?;
    let mut end = first.content_end;
    for line in markdown_line::lines(&content[first.end..]) {
        if !line.text.starts_with([' ', '\t']) {
            break;
        }
        end = first.end + line.content_end;
    }
    Some(first.start..end)
}

fn find_fence(content: &str, start: usize) -> Option<markdown_line::MarkdownLine<'_>> {
    markdown_line::find(content, start, |line| {
        line.strip_suffix('\r')
            .unwrap_or(line)
            .strip_prefix("---")
            .is_some_and(|suffix| {
                suffix
                    .chars()
                    .all(|character| matches!(character, ' ' | '\t'))
            })
    })
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

fn remove_field_block(content: &str, field: &str) -> String {
    let Some(mut range) = find_field_block(content, field) else {
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

pub(super) fn set_blocked_by_text(content: &str, value: Option<&BlockedBy>) -> String {
    set_frontmatter_line(
        content,
        "blocked_by:",
        value.map(|value| format!("blocked_by: {}", blocked_by_frontmatter_value(value))),
    )
}

fn blocked_by_frontmatter_value(blocked_by: &BlockedBy) -> String {
    format!(
        "[{}]",
        blocked_by
            .iter()
            .map(|id| format!("\"[[{id}]]\""))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

pub(super) fn parse_blocked_by(content: &str) -> StoredBlockedBy {
    let Some(raw) = frontmatter_field_value(content, "blocked_by:") else {
        return StoredBlockedBy::Absent;
    };
    let parsed = match Matter::<YAML>::new()
        .parse::<BlockedByFrontmatter>(content.strip_prefix(UTF8_BOM).unwrap_or(content))
    {
        Ok(parsed) => parsed,
        Err(error) => {
            return StoredBlockedBy::Malformed {
                raw,
                reason: error.to_string(),
            };
        }
    };
    let Some(frontmatter) = parsed.data else {
        return malformed_blocked_by(raw, "expected YAML frontmatter");
    };
    let Value::Array(values) = frontmatter.blocked_by else {
        return malformed_blocked_by(raw, "expected a YAML sequence of quoted wikilinks");
    };
    if values.is_empty() {
        return StoredBlockedBy::Absent;
    }

    let mut identifiers = Vec::with_capacity(values.len());
    for value in values {
        let Value::String(value) = value else {
            return malformed_blocked_by(
                raw,
                "expected every blocked_by entry to be a quoted wikilink",
            );
        };
        let Some(identifier) = value
            .strip_prefix("[[")
            .and_then(|value| value.strip_suffix("]]"))
        else {
            return malformed_blocked_by(
                raw,
                "expected every blocked_by entry to be an Obsidian wikilink",
            );
        };
        let Ok(identifier) = identifier.parse::<TaskId>() else {
            return malformed_blocked_by(raw, "blocked_by contains an invalid task ID");
        };
        identifiers.push(identifier);
    }

    match BlockedBy::try_new(identifiers) {
        Ok(blocked_by) => StoredBlockedBy::Valid(blocked_by),
        Err(_) => StoredBlockedBy::Absent,
    }
}

fn frontmatter_field_value(content: &str, field: &str) -> Option<String> {
    let bounds = opening_frontmatter_bounds(content)?;
    let frontmatter = &content[bounds.start..bounds.end];
    let range = find_field_block(frontmatter, field)?;
    frontmatter[range]
        .strip_prefix(field)
        .map(str::trim)
        .map(str::to_string)
}

fn malformed_blocked_by(raw: String, reason: impl Into<String>) -> StoredBlockedBy {
    StoredBlockedBy::Malformed {
        raw,
        reason: reason.into(),
    }
}

pub(super) fn set_completed_text(content: &str, value: Option<&AppDate>) -> String {
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

pub(super) fn set_tags_text(content: &str, value: Option<&TaskTags>) -> String {
    set_frontmatter_line(
        content,
        "tags:",
        value.map(|tags| format!("tags: {}", tags_frontmatter_value(tags))),
    )
}

fn tags_frontmatter_value(tags: &TaskTags) -> String {
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
    use pwf_application::ports::task_record::StoredBlockedBy;
    use pwf_models::{
        AppDate,
        task::{BlockedBy, TaskId, TaskStatus, TaskTitle},
    };

    use super::{
        NewTaskFields, new_task_content, parse_blocked_by, set_blocked_by_text, set_status_text,
    };

    fn blocked_by(ids: &[&str]) -> BlockedBy {
        BlockedBy::try_new(ids.iter().map(|id| id.parse().unwrap()).collect::<Vec<_>>()).unwrap()
    }

    #[test]
    fn new_task_renders_blocked_by_as_a_quoted_wikilink_array() {
        let id = TaskId::try_new("PWF-0003").unwrap();
        let title = TaskTitle::try_new("follow up").unwrap();
        let created = "2026-08-20".parse::<AppDate>().unwrap();
        let blockers = blocked_by(&["pwf1", "AUX-0014"]);

        let note = new_task_content(NewTaskFields {
            id: &id,
            title: &title,
            project: "pwf",
            body: "body",
            created: &created,
            blocked_by: Some(&blockers),
            effort: None,
            tags: None,
        });

        assert!(
            note.contains("blocked_by: [\"[[PWF-0001]]\", \"[[AUX-0014]]\"]\n"),
            "{note}"
        );
    }

    #[test]
    fn blocked_by_edit_replaces_an_existing_block_sequence_without_touching_other_bytes() {
        let source = concat!(
            "---\n",
            "id: PWF-0003\n",
            "blocked_by:\n",
            "  - \"[[PWF-0001]]\"\n",
            "  - \"[[PWF-0002]]\"\n",
            "effort: medium\n",
            "---\n\n",
            "body\n",
        );

        let updated = set_blocked_by_text(source, Some(&blocked_by(&["AUX-0014"])));

        assert_eq!(
            updated,
            concat!(
                "---\n",
                "id: PWF-0003\n",
                "blocked_by: [\"[[AUX-0014]]\"]\n",
                "effort: medium\n",
                "---\n\n",
                "body\n",
            )
        );
    }

    #[test]
    fn blocked_by_parser_accepts_inline_and_block_sequences() {
        for source in [
            "---\nblocked_by: [\"[[PWF-0001]]\", \"[[AUX-0014]]\"]\n---\n",
            "---\nblocked_by:\n  - \"[[PWF-0001]]\"\n  - \"[[AUX-0014]]\"\n---\n",
        ] {
            let StoredBlockedBy::Valid(blocked_by) = parse_blocked_by(source) else {
                panic!("expected valid blocked_by metadata");
            };

            assert_eq!(
                blocked_by.iter().map(AsRef::as_ref).collect::<Vec<_>>(),
                ["PWF-0001", "AUX-0014"]
            );
        }
    }

    #[test]
    fn blocked_by_parser_preserves_a_malformed_scalar_for_boundary_specific_diagnostics() {
        let source = "---\nblocked_by: \"[[PWF-0001]]\"\n---\n";

        let StoredBlockedBy::Malformed { raw, reason } = parse_blocked_by(source) else {
            panic!("expected malformed blocked_by metadata");
        };

        assert_eq!(raw, "\"[[PWF-0001]]\"");
        assert!(reason.contains("sequence"), "{reason}");
    }

    #[test]
    fn close_status_inserts_completion_without_rewriting_other_bytes() {
        let source = "---\nid: PWF-0001\nstatus: active\ntitle: task\n---\n\nbody\n";

        assert_eq!(
            set_status_text(
                source,
                TaskStatus::Done,
                Some(&"2026-07-29".parse::<AppDate>().unwrap()),
            ),
            "---\nid: PWF-0001\nstatus: done\ncompleted: 2026-07-29\ntitle: task\n---\n\nbody\n"
        );
    }
}
