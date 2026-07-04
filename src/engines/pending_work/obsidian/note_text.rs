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
static EFFORT_LINE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^effort:.*$").unwrap());
static EFFORT_LINE_NL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^effort:.*\n?").unwrap());

const REPORT_HEADER: &str = "### Report";

static REPORT_HEADER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^### Report\s*$").unwrap());

/// The four canonical body sections, in [`prompt_lanes::adapters::MarkdownAdapter`]'s
/// render order.
const LANE_SECTION_HEADERS: [&str; 4] =
    ["## Goals", "## Context", "## Constraints", "## Done When"];

/// Any Markdown heading line (H1-H6) — the boundary a spliced-in section stops at, so
/// a trailing `### Report` block never gets swallowed into the section above it.
static HEADING_LINE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^#{1,6}\s.*$").unwrap());

/// Splices rich lane-syntax bullets into a note body's Goals/Context/Constraints/Done
/// When sections — an existing section grows in place; a missing one is created at the
/// end of the note. Unlike [`work_item_content`]'s full-body regeneration, existing
/// bullets and any trailing `### Report` block are left untouched. Used by `update
/// --append`/`-a` (FR-0002). Returns `None` when `prompt` is whitespace-only.
pub fn append_lanes_text(content: &str, prompt: &str) -> Option<String> {
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

/// Appends `bullets` under `header` in `content`, splicing into the section when the
/// header already exists (stopping before the next heading of any level) or creating a
/// fresh section at the end otherwise. A no-op when `bullets` is empty.
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
            out.push_str(&format!("\n- {bullet}"));
        }
        out.push('\n');
        return out;
    };
    let rest = &content[header_match.end()..];
    let section_end = header_match.end()
        + HEADING_LINE_RE
            .find(rest)
            .map(|m| m.start())
            .unwrap_or(rest.len());
    let before = content[..section_end].trim_end_matches('\n');
    let after = content[section_end..].trim_start_matches('\n');
    let mut out = before.to_string();
    for bullet in bullets {
        out.push_str(&format!("\n- {bullet}"));
    }
    if after.is_empty() {
        out.push('\n');
    } else {
        out.push_str("\n\n");
        out.push_str(after);
    }
    out
}

/// For brand-new items `completed` is None.
#[allow(clippy::too_many_arguments)]
pub fn work_item_content(
    title: &str,
    project: &str,
    prompt: &str,
    status: &str,
    created: &str,
    completed: Option<&str>,
    prereq: Option<&str>,
    effort: Option<u8>,
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
    if let Some(e) = effort {
        out.push_str(&format!("effort: {e}\n"));
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
/// `done`/`cancel` close path. For a multi-section closeout report that must keep
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

/// Sets a single frontmatter line: `Some(line)` replaces an existing `line_re`
/// match, or inserts `line` matching [`work_item_content`]'s placement (after
/// `completed:` when present, else after `created:`, else before the closing
/// `---`); `None` deletes the whole line via `line_nl_re` (with its trailing
/// newline, leaving no blank residue).
fn set_frontmatter_line(
    content: &str,
    line_re: &Regex,
    line_nl_re: &Regex,
    line: Option<String>,
) -> String {
    let Some(line) = line else {
        return line_nl_re.replace(content, "").into_owned();
    };
    if line_re.is_match(content) {
        return line_re.replace(content, line.as_str()).into_owned();
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

/// Sets the `prereq:` frontmatter line to `value` (double-quoted, wikilink-shaped),
/// or removes it when `None` — see [`set_frontmatter_line`] for placement.
pub fn set_prereq_text(content: &str, value: Option<&str>) -> String {
    set_frontmatter_line(
        content,
        &PREREQ_LINE_RE,
        &PREREQ_LINE_NL_RE,
        value.map(|v| format!("prereq: \"{v}\"")),
    )
}

/// Sets the `commits:` frontmatter line to `value`, or removes it when `None` —
/// see [`set_frontmatter_line`] for placement. The raw commit range is stored
/// verbatim (double-quoted); a range is provenance, never a wikilink.
pub fn set_commits_text(content: &str, value: Option<&str>) -> String {
    set_frontmatter_line(
        content,
        &COMMITS_LINE_RE,
        &COMMITS_LINE_NL_RE,
        value.map(|v| format!("commits: \"{v}\"")),
    )
}

/// Sets the `effort:` frontmatter line to `value` (1-4), or removes it when `None`
/// — see [`set_frontmatter_line`] for placement. The value is a bare unquoted
/// integer — an effort tier is a plain number, never a wikilink or provenance range.
pub fn set_effort_text(content: &str, value: Option<u8>) -> String {
    set_frontmatter_line(
        content,
        &EFFORT_LINE_RE,
        &EFFORT_LINE_NL_RE,
        value.map(|v| format!("effort: {v}")),
    )
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
    fn append_lanes_splices_into_an_existing_section() {
        let content = "---\nstatus: active\n---\n\n## Goals\n- x\n";
        let got = append_lanes_text(content, "more work").unwrap();
        assert_eq!(
            got,
            "---\nstatus: active\n---\n\n## Goals\n- x\n- more work\n"
        );
    }

    #[test]
    fn append_lanes_creates_a_missing_section_and_keeps_goals_intact() {
        let content = "---\nstatus: active\n---\n\n## Goals\n- x\n";
        let got = append_lanes_text(content, "some title /c new context").unwrap();
        assert_eq!(
            got,
            "---\nstatus: active\n---\n\n## Goals\n- x\n- some title\n\n## Context\n- new context\n"
        );
    }

    #[test]
    fn append_lanes_populates_all_four_sections_in_canonical_order() {
        let content = "---\nstatus: active\n---\n\n## Goals\n- x\n";
        let prompt = "another goal /c more context /n a constraint /d a done condition";
        let got = append_lanes_text(content, prompt).unwrap();
        assert_eq!(
            got,
            "---\nstatus: active\n---\n\n## Goals\n- x\n- another goal\n\n## Context\n- more context\n\n## Constraints\n- a constraint\n\n## Done When\n- a done condition\n"
        );
    }

    #[test]
    fn append_lanes_stops_before_a_trailing_report_block() {
        // A closeout report may already sit below the body; a new Goals bullet must
        // land above it, not get swallowed into the section it follows.
        let content = "---\nstatus: active\n---\n\n## Goals\n- x\n\n### Report\n\nsome note\n";
        let got = append_lanes_text(content, "more work").unwrap();
        assert_eq!(
            got,
            "---\nstatus: active\n---\n\n## Goals\n- x\n- more work\n\n### Report\n\nsome note\n"
        );
    }

    #[test]
    fn append_lanes_rejects_whitespace_only_prompt() {
        let content = "---\nstatus: active\n---\n\n## Goals\n- x\n";
        assert!(append_lanes_text(content, "   \n\t").is_none());
    }

    #[test]
    fn append_report_block_extends_existing_report_section() {
        // An item closed with `done --report` already carries a one-line `### Report`;
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
            None,
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
            None,
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

    #[test]
    fn work_item_content_writes_effort_when_present() {
        let content = work_item_content(
            "t",
            "glep-shimeji",
            "body",
            "active",
            "2026-01-01",
            None,
            None,
            Some(3),
        );
        assert!(content.contains("effort: 3\n"), "got: {content}");
    }

    #[test]
    fn work_item_content_omits_effort_when_absent() {
        let content = work_item_content(
            "t",
            "glep-shimeji",
            "body",
            "active",
            "2026-01-01",
            None,
            None,
            None,
        );
        assert!(!content.contains("effort:"), "got: {content}");
    }

    #[test]
    fn set_effort_inserts_when_absent() {
        let content = work_item_content(
            "t",
            "glep-shimeji",
            "body",
            "active",
            "2026-01-01",
            None,
            None,
            None,
        );
        let got = set_effort_text(&content, Some(4));
        assert!(got.contains("effort: 4\n"), "effort line missing: {got}");
    }

    #[test]
    fn set_effort_replaces_existing() {
        let content = work_item_content(
            "t",
            "glep-shimeji",
            "body",
            "active",
            "2026-01-01",
            None,
            None,
            Some(1),
        );
        let got = set_effort_text(&content, Some(2));
        assert_eq!(got.matches("effort:").count(), 1, "duplicate effort: {got}");
        assert!(got.contains("effort: 2"), "got: {got}");
    }

    #[test]
    fn set_effort_none_removes_line() {
        let content = work_item_content(
            "t",
            "glep-shimeji",
            "body",
            "active",
            "2026-01-01",
            None,
            None,
            Some(2),
        );
        let got = set_effort_text(&content, None);
        assert!(!got.contains("effort:"), "effort line lingered: {got}");
    }
}
