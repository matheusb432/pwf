// Work-item note rendering and frontmatter string transforms.

use std::{fmt::Write, sync::LazyLock};

use regex::Regex;

static COMPLETED_LINE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^completed:.*$").unwrap());

/// The fields rendered into a work-item note body by [`work_item_content`].
#[derive(Clone, Copy)]
pub struct WorkItemFields<'a> {
    pub title: &'a str,
    pub project: &'a str,
    pub prompt: &'a str,
    pub status: &'a str,
    pub created: &'a str,
    pub completed: Option<&'a str>,
    pub prereq: Option<&'a str>,
    pub effort: Option<u8>,
}

/// For brand-new items `completed` is None.
pub fn work_item_content(fields: WorkItemFields<'_>) -> String {
    let WorkItemFields {
        title,
        project,
        prompt,
        status,
        created,
        completed,
        prereq,
        effort,
    } = fields;
    let mut out = String::new();
    out.push_str("---\n");
    let _ = writeln!(out, "status: {status}");
    let _ = writeln!(out, "title: {title}");
    let _ = writeln!(out, "project: {project}");
    let _ = writeln!(out, "created: {created}");
    if let Some(c) = completed {
        let _ = writeln!(out, "completed: {c}");
    }
    if let Some(p) = prereq {
        let _ = writeln!(out, "prereq: \"{p}\"");
    }
    if let Some(e) = effort {
        let _ = writeln!(out, "effort: {e}");
    }
    out.push_str("---\n\n");
    out.push_str(prompt.trim_end());
    out.push('\n');
    out
}

/// Replace first `status:` line; insert/replace `completed:` (insert right after
/// the new status line when absent).
///
/// # Panics
/// Panics if the internal status-matching regex fails to compile — unreachable
/// in practice since `status` is escaped via [`regex::escape`].
pub fn set_status_text(content: &str, status: &str, completed: &str) -> String {
    let c = crate::regexes::STATUS_LINE_RE
        .replace(content, format!("status: {status}").as_str())
        .into_owned();
    if COMPLETED_LINE_RE.is_match(&c) {
        COMPLETED_LINE_RE
            .replace(&c, format!("completed: {completed}").as_str())
            .into_owned()
    } else {
        let after = Regex::new(&format!(r"(?m)^(status: {})$", regex::escape(status))).unwrap();
        after
            .replace(
                &c,
                format!("status: {status}\ncompleted: {completed}").as_str(),
            )
            .into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_item_content_writes_effort_when_present() {
        let content = work_item_content(WorkItemFields {
            title: "t",
            project: "glep-shimeji",
            prompt: "body",
            status: "active",
            created: "2026-01-01",
            completed: None,
            prereq: None,
            effort: Some(3),
        });
        assert!(content.contains("effort: 3\n"), "got: {content}");
    }

    #[test]
    fn work_item_content_omits_effort_when_absent() {
        let content = work_item_content(WorkItemFields {
            title: "t",
            project: "glep-shimeji",
            prompt: "body",
            status: "active",
            created: "2026-01-01",
            completed: None,
            prereq: None,
            effort: None,
        });
        assert!(!content.contains("effort:"), "got: {content}");
    }
}
