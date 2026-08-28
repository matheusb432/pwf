use std::fmt::Write as _;

use pwf_application::ports::task_record::StoredBlockedBy;
use pwf_models::{
    AppDate,
    task::{BlockedBy, EffortTier, TaskId, TaskStatus, TaskTags, TaskTitle},
};
use serde::Deserialize;
use serde_json::Value;

use super::{FrontmatterView, MarkdownFile, MarkdownFileError};

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

pub(super) fn set_status(
    file: &mut MarkdownFile,
    status: TaskStatus,
    completed: Option<&AppDate>,
) -> Result<(), MarkdownFileError> {
    if file.property_text("status")?.is_none() {
        return Ok(());
    }
    file.set_property_rendered("status", Some(status.as_str()), &[])?;
    let completed = completed.map(ToString::to_string).unwrap_or_default();
    file.set_property_rendered("completed", Some(&completed), &["status"])?;
    Ok(())
}

pub(super) fn reopen_status(file: &mut MarkdownFile) -> Result<(), MarkdownFileError> {
    if file.property_text("status")?.is_some() {
        file.set_property_rendered("status", Some("active"), &[])?;
    }
    file.remove_property("completed")?;
    Ok(())
}

pub(super) fn set_blocked_by(
    file: &mut MarkdownFile,
    value: Option<&BlockedBy>,
) -> Result<(), MarkdownFileError> {
    let rendered = value.map(blocked_by_frontmatter_value);
    file.set_property_rendered("blocked_by", rendered.as_deref(), &["completed", "created"])?;
    Ok(())
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

pub(super) fn parse_blocked_by(frontmatter: Option<&FrontmatterView<'_>>) -> StoredBlockedBy {
    let Some(frontmatter) = frontmatter else {
        return StoredBlockedBy::Absent;
    };
    let raw = match frontmatter.get("blocked_by") {
        Ok(Some(raw)) => raw.to_string(),
        Ok(None) => return StoredBlockedBy::Absent,
        Err(error) => return malformed_blocked_by(String::new(), error.to_string()),
    };
    let frontmatter = match frontmatter.deserialize::<BlockedByFrontmatter>() {
        Ok(frontmatter) => frontmatter,
        Err(error) => return malformed_blocked_by(raw, error.to_string()),
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

fn malformed_blocked_by(raw: String, reason: impl Into<String>) -> StoredBlockedBy {
    StoredBlockedBy::Malformed {
        raw,
        reason: reason.into(),
    }
}

pub(super) fn set_completed(
    file: &mut MarkdownFile,
    value: Option<&AppDate>,
) -> Result<(), MarkdownFileError> {
    let rendered = value.map(ToString::to_string);
    file.set_property_rendered("completed", rendered.as_deref(), &["created"])?;
    Ok(())
}

pub(super) fn set_commits(
    file: &mut MarkdownFile,
    value: Option<&str>,
) -> Result<(), MarkdownFileError> {
    match value {
        Some(value) => {
            file.set_property_after("commits", value, &["completed", "created"])?;
        }
        None => {
            file.remove_property("commits")?;
        }
    }
    Ok(())
}

pub(super) fn set_effort(
    file: &mut MarkdownFile,
    value: Option<EffortTier>,
) -> Result<(), MarkdownFileError> {
    let rendered = value.map(|value| value.to_string());
    file.set_property_rendered("effort", rendered.as_deref(), &["completed", "created"])?;
    Ok(())
}

pub(super) fn set_tags(
    file: &mut MarkdownFile,
    value: Option<&TaskTags>,
) -> Result<(), MarkdownFileError> {
    let rendered = value.map(tags_frontmatter_value);
    file.set_property_rendered("tags", rendered.as_deref(), &["completed", "created"])?;
    Ok(())
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
    use std::path::Path;

    use pwf_application::ports::task_record::StoredBlockedBy;
    use pwf_models::{
        AppDate,
        task::{BlockedBy, TaskId, TaskStatus, TaskTitle},
    };

    use super::{NewTaskFields, new_task_content, parse_blocked_by, set_blocked_by, set_status};
    use crate::obsidian::MarkdownFile;

    fn blocked_by(ids: &[&str]) -> BlockedBy {
        BlockedBy::try_new(ids.iter().map(|id| id.parse().unwrap()).collect::<Vec<_>>()).unwrap()
    }

    fn file(source: &str) -> MarkdownFile {
        MarkdownFile::from_source(Path::new("task.md").to_path_buf(), source.to_string())
    }

    fn parsed_blocked_by(file: &MarkdownFile) -> StoredBlockedBy {
        parse_blocked_by(file.frontmatter_view().unwrap().as_ref())
    }

    fn valid_blocked_by(file: &MarkdownFile) -> BlockedBy {
        let blocked_by = match parsed_blocked_by(file) {
            StoredBlockedBy::Valid(blocked_by) => Some(blocked_by),
            StoredBlockedBy::Absent | StoredBlockedBy::Malformed { .. } => None,
        };
        assert!(blocked_by.is_some());
        blocked_by.unwrap()
    }

    fn malformed_blocked_by(file: &MarkdownFile) -> (String, String) {
        let malformed = match parsed_blocked_by(file) {
            StoredBlockedBy::Malformed { raw, reason } => Some((raw, reason)),
            StoredBlockedBy::Absent | StoredBlockedBy::Valid(_) => None,
        };
        assert!(malformed.is_some());
        malformed.unwrap()
    }

    #[test]
    fn new_task_renders_blocked_by_as_a_quoted_wikilink_array() {
        let id = TaskId::try_new("FOO-0003").unwrap();
        let title = TaskTitle::try_new("follow up").unwrap();
        let created = "2026-08-20".parse::<AppDate>().unwrap();
        let blockers = blocked_by(&["foo1", "AUX-0014"]);

        let note = new_task_content(NewTaskFields {
            id: &id,
            title: &title,
            project: "foo",
            body: "body",
            created: &created,
            blocked_by: Some(&blockers),
            effort: None,
            tags: None,
        });

        assert!(
            note.contains("blocked_by: [\"[[FOO-0001]]\", \"[[AUX-0014]]\"]\n"),
            "{note}"
        );
    }

    #[test]
    fn blocked_by_edit_replaces_an_existing_block_sequence_without_touching_other_bytes() {
        let mut file = file(concat!(
            "---\n",
            "id: FOO-0003\n",
            "blocked_by:\n",
            "  - \"[[FOO-0001]]\"\n",
            "  - \"[[FOO-0002]]\"\n",
            "effort: medium\n",
            "---\n\n",
            "body\n",
        ));

        set_blocked_by(&mut file, Some(&blocked_by(&["AUX-0014"]))).unwrap();

        assert_eq!(
            file.source(),
            concat!(
                "---\n",
                "id: FOO-0003\n",
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
            "---\nblocked_by: [\"[[FOO-0001]]\", \"[[AUX-0014]]\"]\n---\n",
            "---\nblocked_by:\n  - \"[[FOO-0001]]\"\n  - \"[[AUX-0014]]\"\n---\n",
        ] {
            let file = file(source);
            let blocked_by = valid_blocked_by(&file);

            assert_eq!(
                blocked_by.iter().map(AsRef::as_ref).collect::<Vec<_>>(),
                ["FOO-0001", "AUX-0014"]
            );
        }
    }

    #[test]
    fn blocked_by_parser_preserves_a_malformed_scalar_for_boundary_specific_diagnostics() {
        let file = file("---\nblocked_by: \"[[FOO-0001]]\"\n---\n");

        let (raw, reason) = malformed_blocked_by(&file);

        assert_eq!(raw, "\"[[FOO-0001]]\"");
        assert!(reason.contains("sequence"), "{reason}");
    }

    #[test]
    fn close_status_inserts_completion_without_rewriting_other_bytes() {
        let mut file = file("---\nid: FOO-0001\nstatus: active\ntitle: task\n---\n\nbody\n");

        set_status(
            &mut file,
            TaskStatus::Done,
            Some(&"2026-07-29".parse::<AppDate>().unwrap()),
        )
        .unwrap();

        assert_eq!(
            file.source(),
            "---\nid: FOO-0001\nstatus: done\ncompleted: 2026-07-29\ntitle: task\n---\n\nbody\n"
        );
    }
}
