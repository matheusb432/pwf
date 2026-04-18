// Work-item note rendering and frontmatter string transforms.

use regex::Regex;

const REPORT_HEADER: &str = "### Report";

/// For brand-new items `completed` is None.
pub fn work_item_content(
    title: &str,
    project: &str,
    prompt: &str,
    status: &str,
    created: &str,
    completed: Option<&str>,
    prereq: Option<&str>,
) -> String {
    let mut out = String::new();
    out.push_str("---\n");
    out.push_str(&format!("status: {status}\n"));
    out.push_str(&format!("title: {title}\n"));
    out.push_str(&format!("project: {project}\n"));
    out.push_str(&format!("created: {created}\n"));
    if let Some(c) = completed {
        out.push_str(&format!("completed: {c}\n"));
    }
    if let Some(p) = prereq {
        out.push_str(&format!("prereq: \"{p}\"\n"));
    }
    out.push_str("---\n\n");
    out.push_str(prompt.trim_end());
    out.push('\n');
    out
}

fn normalized_report(report: &str) -> Option<String> {
    let mut parts = Vec::new();
    for line in report.lines() {
        let line = line.trim();
        if !line.is_empty() {
            parts.push(line);
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" "))
    }
}

/// Append the standard completion report section to a work-item note.
pub fn append_report_text(content: &str, report: &str) -> Option<String> {
    let report = normalized_report(report)?;
    let mut out = content.trim_end().to_string();
    out.push_str("\n\n");
    out.push_str(REPORT_HEADER);
    out.push_str("\n\n");
    out.push_str(&report);
    out.push('\n');
    Some(out)
}

/// Replace first `status:` line; insert/replace `completed:` (insert right after
/// the new status line when absent).
pub fn set_status_text(content: &str, status: &str, completed: &str) -> String {
    let status_re = Regex::new(r"(?m)^status:.*$").unwrap();
    let c = status_re
        .replace(content, format!("status: {status}").as_str())
        .into_owned();
    let completed_re = Regex::new(r"(?m)^completed:.*$").unwrap();
    if completed_re.is_match(&c) {
        completed_re
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

/// Sets the `prereq:` frontmatter line to `value`, or removes it when `None`.
///
/// `Some(v)` replaces an existing `prereq:` line, or inserts `prereq: "{v}"` matching
/// [`work_item_content`]'s placement (after `completed:`, else after `created:`, else
/// before the closing `---`). `None` deletes the whole line, leaving no `prereq: ""`
/// residue.
pub fn set_prereq_text(content: &str, value: Option<&str>) -> String {
    let prereq_re = Regex::new(r"(?m)^prereq:.*$").unwrap();
    let Some(v) = value else {
        // Drop the line and its trailing newline so no blank line is left behind.
        let drop_re = Regex::new(r"(?m)^prereq:.*\n?").unwrap();
        return drop_re.replace(content, "").into_owned();
    };
    let line = format!("prereq: \"{v}\"");
    if prereq_re.is_match(content) {
        return prereq_re.replace(content, line.as_str()).into_owned();
    }
    // Anchor after `completed:` (when present), else after `created:`.
    for anchor in [r"(?m)^completed:.*$", r"(?m)^created:.*$"] {
        let re = Regex::new(anchor).unwrap();
        if let Some(m) = re.find(content) {
            return format!("{}\n{line}{}", &content[..m.end()], &content[m.end()..]);
        }
    }
    // Fallback: insert before the closing `---`.
    let close_re = Regex::new(r"(?m)^---[ \t]*$").unwrap();
    let mut fences = close_re.find_iter(content);
    if let (Some(_), Some(close)) = (fences.next(), fences.next()) {
        return format!(
            "{}{line}\n{}",
            &content[..close.start()],
            &content[close.start()..]
        );
    }
    content.to_string()
}

/// Sets the `commits:` frontmatter line to `value`, or removes it when `None`.
///
/// Mirrors [`set_prereq_text`]'s placement (after `completed:`, else after `created:`,
/// else before the closing `---`), but stores the raw commit range verbatim; a range
/// is provenance, never a wikilink, so the value is never wrapped.
pub fn set_commits_text(content: &str, value: Option<&str>) -> String {
    let commits_re = Regex::new(r"(?m)^commits:.*$").unwrap();
    let Some(v) = value else {
        let drop_re = Regex::new(r"(?m)^commits:.*\n?").unwrap();
        return drop_re.replace(content, "").into_owned();
    };
    let line = format!("commits: \"{v}\"");
    if commits_re.is_match(content) {
        return commits_re.replace(content, line.as_str()).into_owned();
    }
    for anchor in [r"(?m)^completed:.*$", r"(?m)^created:.*$"] {
        let re = Regex::new(anchor).unwrap();
        if let Some(m) = re.find(content) {
            return format!("{}\n{line}{}", &content[..m.end()], &content[m.end()..]);
        }
    }
    let close_re = Regex::new(r"(?m)^---[ \t]*$").unwrap();
    let mut fences = close_re.find_iter(content);
    if let (Some(_), Some(close)) = (fences.next(), fences.next()) {
        return format!(
            "{}{line}\n{}",
            &content[..close.start()],
            &content[close.start()..]
        );
    }
    content.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_report_text_adds_standard_section_and_collapses_lines() {
        let content = "---\nstatus: active\n---\n\nbody\n";
        assert_eq!(
            append_report_text(content, "  did one thing\nand another  ").unwrap(),
            "---\nstatus: active\n---\n\nbody\n\n### Report\n\ndid one thing and another\n"
        );
        assert!(append_report_text(content, " \n ").is_none());
    }

    #[test]
    fn set_commits_inserts_when_absent_storing_value_raw() {
        let content = work_item_content(
            "t",
            "glep-shimeji",
            "body",
            "active",
            "2026-01-01",
            None,
            None,
        );
        let got = set_commits_text(&content, Some("a1b2c3d..f4e5d6c"));
        assert!(
            got.contains("commits: \"a1b2c3d..f4e5d6c\""),
            "commits line missing: {got}"
        );
        // Raw range, never wikilink-wrapped.
        assert!(!got.contains("[["), "commits must not be wikilinked: {got}");
    }

    #[test]
    fn set_commits_replaces_existing() {
        let content = "---\nstatus: active\ncreated: 2026-01-01\ncommits: \"a..b\"\n---\n\nbody\n";
        let got = set_commits_text(content, Some("c..d"));
        assert_eq!(
            got.matches("commits:").count(),
            1,
            "duplicate commits: {got}"
        );
        assert!(got.contains("commits: \"c..d\""), "got: {got}");
    }

    #[test]
    fn set_commits_none_removes_line() {
        let content = "---\nstatus: active\ncreated: 2026-01-01\ncommits: \"a..b\"\n---\n\nbody\n";
        let got = set_commits_text(content, None);
        assert!(!got.contains("commits:"), "commits line lingered: {got}");
    }

    #[test]
    fn set_prereq_inserts_when_absent() {
        let content = work_item_content(
            "t",
            "glep-shimeji",
            "body",
            "active",
            "2026-01-01",
            None,
            None,
        );
        let got = set_prereq_text(&content, Some("[[GLP-0001]]"));
        assert!(
            got.contains("prereq: \"[[GLP-0001]]\""),
            "prereq line missing: {got}"
        );
        let parsed = crate::frontmatter::parse(&got);
        // The parser preserves the literal double-quoted value (matches add's output).
        assert_eq!(
            parsed.frontmatter.get("prereq").map(String::as_str),
            Some("\"[[GLP-0001]]\"")
        );
    }

    #[test]
    fn set_prereq_replaces_existing() {
        let content = work_item_content(
            "t",
            "glep-shimeji",
            "body",
            "active",
            "2026-01-01",
            None,
            Some("[[GLP-0001]]"),
        );
        let got = set_prereq_text(&content, Some("[[GLP-0002]]"));
        assert_eq!(got.matches("prereq:").count(), 1, "duplicate prereq: {got}");
        let parsed = crate::frontmatter::parse(&got);
        assert_eq!(
            parsed.frontmatter.get("prereq").map(String::as_str),
            Some("\"[[GLP-0002]]\"")
        );
    }

    #[test]
    fn set_prereq_none_removes_line() {
        let content = work_item_content(
            "t",
            "glep-shimeji",
            "body",
            "active",
            "2026-01-01",
            None,
            Some("[[GLP-0001]]"),
        );
        let got = set_prereq_text(&content, None);
        assert!(!got.contains("prereq:"), "prereq line lingered: {got}");
        let parsed = crate::frontmatter::parse(&got);
        assert_eq!(
            parsed.frontmatter.get("status").map(String::as_str),
            Some("active")
        );
        assert_eq!(
            parsed.frontmatter.get("title").map(String::as_str),
            Some("t")
        );
        assert_eq!(
            parsed.frontmatter.get("project").map(String::as_str),
            Some("glep-shimeji")
        );
        assert_eq!(
            parsed.frontmatter.get("created").map(String::as_str),
            Some("2026-01-01")
        );
    }
}
