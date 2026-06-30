// Work-item note rendering and frontmatter string transforms.

use std::sync::LazyLock;

use regex::Regex;

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

const REPORT_HEADER: &str = "### Report";

static REPORT_HEADER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^### Report\s*$").unwrap());

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
///
/// Collapses the report to a single normalized line (BR-0005); used by the
/// `check`/`cancel` close path. For a multi-section closeout report that must keep
/// its Markdown structure, use [`append_report_block_text`] instead.
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

/// Append a free-form, multi-line Markdown closeout report to a note's body
/// **verbatim** — newlines, headings, lists, and blank lines are preserved.
///
/// Unlike [`append_report_text`] (which collapses to one line for the close path),
/// this is the `update --append-report` path for attaching a narrative report to an
/// already-closed item without rerunning title/Goals regeneration. Returns `None`
/// when the report is whitespace-only. When the note already has a `### Report`
/// section the verbatim block is appended after it (separated by a blank line);
/// otherwise a fresh `### Report` header is created.
pub fn append_report_block_text(content: &str, report: &str) -> Option<String> {
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

/// Replace first `status:` line; insert/replace `completed:` (insert right after
/// the new status line when absent).
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

/// Reopen a closed note: flip `status:` back to `active` and drop the `completed:`
/// line (with its newline, leaving no blank residue). The inverse of
/// [`set_status_text`]; `commits:` is dropped separately via
/// [`set_commits_text`]`(.., None)`.
pub fn reopen_status_text(content: &str) -> String {
    let c = crate::regexes::STATUS_LINE_RE
        .replace(content, "status: active")
        .into_owned();
    COMPLETED_LINE_NL_RE.replace(&c, "").into_owned()
}

/// Sets the `prereq:` frontmatter line to `value`, or removes it when `None`.
///
/// `Some(v)` replaces an existing `prereq:` line, or inserts `prereq: "{v}"` matching
/// [`work_item_content`]'s placement (after `completed:`, else after `created:`, else
/// before the closing `---`). `None` deletes the whole line, leaving no `prereq: ""`
/// residue.
pub fn set_prereq_text(content: &str, value: Option<&str>) -> String {
    let Some(v) = value else {
        // Drop the line and its trailing newline so no blank line is left behind.
        return PREREQ_LINE_NL_RE.replace(content, "").into_owned();
    };
    let line = format!("prereq: \"{v}\"");
    if PREREQ_LINE_RE.is_match(content) {
        return PREREQ_LINE_RE.replace(content, line.as_str()).into_owned();
    }
    // Anchor after `completed:` (when present), else after `created:`.
    for re in [&*COMPLETED_LINE_RE, &*CREATED_LINE_RE] {
        if let Some(m) = re.find(content) {
            return format!("{}\n{line}{}", &content[..m.end()], &content[m.end()..]);
        }
    }
    // Fallback: insert before the closing `---`.
    let mut fences = crate::regexes::FRONTMATTER_FENCE_RE.find_iter(content);
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
    let Some(v) = value else {
        return COMMITS_LINE_NL_RE.replace(content, "").into_owned();
    };
    let line = format!("commits: \"{v}\"");
    if COMMITS_LINE_RE.is_match(content) {
        return COMMITS_LINE_RE.replace(content, line.as_str()).into_owned();
    }
    for re in [&*COMPLETED_LINE_RE, &*CREATED_LINE_RE] {
        if let Some(m) = re.find(content) {
            return format!("{}\n{line}{}", &content[..m.end()], &content[m.end()..]);
        }
    }
    let mut fences = crate::regexes::FRONTMATTER_FENCE_RE.find_iter(content);
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
    fn append_report_block_preserves_multiline_markdown_verbatim() {
        let content = "---\nstatus: done\ntitle: t\n---\n\n## Goals\n\n- ship it\n";
        let report = "## Outcome\n\nShipped `--append-report`.\n\n## Follow-ups\n\n- write the FSD card\n- 50% faster";
        let got = append_report_block_text(content, report).unwrap();
        assert_eq!(
            got,
            "---\nstatus: done\ntitle: t\n---\n\n## Goals\n\n- ship it\n\n### Report\n\n## Outcome\n\nShipped `--append-report`.\n\n## Follow-ups\n\n- write the FSD card\n- 50% faster\n"
        );
        // The original body section is untouched.
        assert!(
            got.contains("## Goals\n\n- ship it\n"),
            "body altered: {got}"
        );
    }

    #[test]
    fn append_report_block_rejects_whitespace_only() {
        let content = "---\nstatus: done\n---\n\nbody\n";
        assert!(append_report_block_text(content, "  \n\t\n ").is_none());
    }

    #[test]
    fn append_report_block_extends_existing_report_section() {
        // An item closed with `check --report` already carries a one-line `### Report`;
        // a later closeout append lands under that same section, not a duplicate header.
        let content = "---\nstatus: done\n---\n\nbody\n\n### Report\n\none-line close note\n";
        let got = append_report_block_text(content, "## Detail\n\nfull writeup").unwrap();
        assert_eq!(
            got,
            "---\nstatus: done\n---\n\nbody\n\n### Report\n\none-line close note\n\n## Detail\n\nfull writeup\n"
        );
        assert_eq!(
            REPORT_HEADER_RE.find_iter(&got).count(),
            1,
            "duplicate ### Report header: {got}"
        );
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
    fn reopen_status_flips_to_active_and_drops_completed() {
        let content = "---\nstatus: done\ncompleted: 2026-01-02\ntitle: t\ncreated: 2026-01-01\ncommits: \"a..b\"\n---\n\nbody\n";
        let got = reopen_status_text(content);
        assert!(got.contains("status: active"), "status not flipped: {got}");
        assert!(!got.contains("status: done"), "done lingered: {got}");
        assert!(!got.contains("completed:"), "completed lingered: {got}");
        // No blank line left where `completed:` was.
        assert!(
            got.contains("status: active\ntitle: t\n"),
            "blank residue: {got}"
        );
        // `commits:` is dropped separately, not by this transform.
        assert!(
            got.contains("commits:"),
            "commits should be untouched: {got}"
        );
    }

    #[test]
    fn reopen_status_without_completed_line_is_idempotent_on_status() {
        let content = "---\nstatus: cancelled\ntitle: t\ncreated: 2026-01-01\n---\n\nbody\n";
        let got = reopen_status_text(content);
        assert!(got.contains("status: active"));
        assert!(!got.contains("status: cancelled"));
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
