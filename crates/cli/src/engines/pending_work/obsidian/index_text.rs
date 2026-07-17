use std::sync::LazyLock;

pub use pwf_core::index::edit::{find_section_index, remove_index_link};
use regex::Regex;

use super::super::section::Section;

static SECTION_MARK_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^##\s").unwrap());
static NOTES_HEADER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^###\s+Notes\s*$").unwrap());
static ANCHOR_WIKILINK_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^- \[\[").unwrap());
static ANCHOR_CHECKBOX_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^- \[").unwrap());

/// Inserts a link at the top of the unheaded task region, after any H1 or blockquote preamble.
/// The region ends before the first H2 or `### Notes`, preventing insertion into sections or note
/// lists.
pub fn add_link_to_index(content: &str, link: &str) -> String {
    let block = format!("{link}\n");

    let normal_end = [
        SECTION_MARK_RE.find(content).map(|m| m.start()),
        NOTES_HEADER_RE.find(content).map(|m| m.start()),
    ]
    .into_iter()
    .flatten()
    .min()
    .unwrap_or(content.len());
    let region = &content[..normal_end];

    let insert_at = ANCHOR_WIKILINK_RE
        .find(region)
        .or_else(|| ANCHOR_CHECKBOX_RE.find(region))
        .map_or(normal_end, |m| m.start());

    let prefix = content[..insert_at].trim_end();
    let suffix = &content[insert_at..];
    // Separate headings with a blank line; keep list items adjacent.
    let sep = if !suffix.is_empty() && suffix.trim_start().starts_with('#') {
        "\n"
    } else {
        ""
    };
    if prefix.is_empty() {
        return format!("{block}{sep}{suffix}");
    }
    format!("{prefix}\n\n{block}{sep}{suffix}")
}

pub fn section_exists(content: &str, section: Section) -> bool {
    find_section_index(content, section.read_headers()).is_some()
}

fn line_end_after(content: &str, idx: usize) -> usize {
    content[idx..]
        .find('\n')
        .map_or(content.len(), |i| idx + i + 1)
}

fn skip_blank_lines(content: &str, mut idx: usize) -> usize {
    while idx < content.len() {
        let rest = &content[idx..];
        let line_len = rest.find('\n').map_or(rest.len(), |i| i + 1);
        let line = &rest[..line_len];
        if !line.trim().is_empty() {
            break;
        }
        idx += line_len;
    }
    idx
}

fn insert_section_item(content: &str, insert_at: usize, block: &str) -> String {
    let suffix_at = skip_blank_lines(content, insert_at);
    let prefix = content[..insert_at].trim_end();
    let suffix = &content[suffix_at..];
    if suffix.is_empty() {
        format!("{prefix}\n{block}")
    } else {
        format!("{prefix}\n{block}\n{suffix}")
    }
}

/// Inserts a task into its section, creating the header when absent.
/// New sections follow Low-prio, Human, Future order; legacy Future headers remain readable.
pub fn add_section_block(content: &str, block: &str, section: Section) -> String {
    let headers = section.read_headers();
    let section_idx = find_section_index(content, headers);

    if let Some(idx) = section_idx {
        return insert_section_item(content, line_end_after(content, idx), block);
    }

    let header = format!("## {}", section.as_str());

    if section == Section::Future {
        let prefix = content.trim_end();
        return format!("{prefix}\n\n{header}\n\n{block}");
    }

    let futuro_idx = find_section_index(content, Section::Future.read_headers());
    if let Some(fidx) = futuro_idx {
        let prefix = content[..fidx].trim_end();
        let suffix = &content[fidx..];
        return format!("{prefix}\n\n{header}\n\n{block}{suffix}");
    }

    let prefix = content.trim_end();
    format!("{prefix}\n\n{header}\n\n{block}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const NEW: &str = "- [ ] [[GLP-0003|new task]]";

    #[test]
    fn add_link_lands_in_normal_region_not_future() {
        let content = "## Future\n- [ ] [[GLP-0007|later]]\n";
        assert_eq!(
            add_link_to_index(content, NEW),
            "- [ ] [[GLP-0003|new task]]\n\n## Future\n- [ ] [[GLP-0007|later]]\n"
        );
    }

    #[test]
    fn add_link_below_leading_h1() {
        assert_eq!(
            add_link_to_index("# glep-shimeji\n", NEW),
            "# glep-shimeji\n\n- [ ] [[GLP-0003|new task]]\n"
        );
    }

    #[test]
    fn add_link_after_leading_blockquote() {
        let content = "> [!note] callout\n\n- [ ] [[GLP-0007|existing]]\n";
        assert_eq!(
            add_link_to_index(content, NEW),
            "> [!note] callout\n\n- [ ] [[GLP-0003|new task]]\n- [ ] [[GLP-0007|existing]]\n"
        );
    }

    #[test]
    fn add_link_newest_on_top_of_normal_items() {
        let content = "- [ ] [[GLP-0001|first]]\n- [ ] [[GLP-0002|second]]\n";
        assert_eq!(
            add_link_to_index(content, NEW),
            "- [ ] [[GLP-0003|new task]]\n- [ ] [[GLP-0001|first]]\n- [ ] [[GLP-0002|second]]\n"
        );
    }

    #[test]
    fn add_link_into_empty_index() {
        assert_eq!(add_link_to_index("", NEW), "- [ ] [[GLP-0003|new task]]\n");
    }

    #[test]
    fn add_section_block_prepends_into_existing_human() {
        let content = "- [ ] [[X-0001|a]]\n\n## Human\n- [ ] [[X-0002|h]]\n";
        let got = add_section_block(content, "- [ ] [[X-0003|new]]\n", Section::Human);
        assert_eq!(
            got,
            "- [ ] [[X-0001|a]]\n\n## Human\n- [ ] [[X-0003|new]]\n\n- [ ] [[X-0002|h]]\n"
        );
        assert_eq!(got.matches("## Human").count(), 1);
    }

    #[test]
    fn add_section_block_inserts_immediately_after_existing_section_header() {
        for (section, header, existing) in [
            (Section::Human, "## Human", "- [ ] [[X-0002|h]]"),
            (Section::Future, "## Future", "- [ ] [[X-0002|f]]"),
            (Section::LowPrio, "## Low-prio", "- [ ] [[X-0002|l]]"),
        ] {
            let content = format!(
                "- [ ] [[X-0001|a]]\n\n{header}\n\n{existing}\n\n### Notes\nkeep these notes\n"
            );
            let got = add_section_block(&content, "- [ ] [[X-0003|new]]\n", section);
            let expected = format!(
                "- [ ] [[X-0001|a]]\n\n{header}\n- [ ] [[X-0003|new]]\n\n{existing}\n\n### Notes\nkeep these notes\n"
            );
            assert_eq!(got, expected, "section {section:?}");
        }
    }

    #[test]
    fn add_section_block_creates_human_before_future() {
        let content = "- [ ] [[X-0001|a]]\n\n## Future\n- [ ] [[X-0002|f]]\n";
        assert_eq!(
            add_section_block(content, "- [ ] [[X-0003|new]]\n", Section::Human),
            "- [ ] [[X-0001|a]]\n\n## Human\n\n- [ ] [[X-0003|new]]\n## Future\n- [ ] [[X-0002|f]]\n"
        );
    }

    #[test]
    fn add_section_block_creates_human_at_end_when_no_future() {
        let content = "- [ ] [[X-0001|a]]\n";
        assert_eq!(
            add_section_block(content, "- [ ] [[X-0003|new]]\n", Section::Human),
            "- [ ] [[X-0001|a]]\n\n## Human\n\n- [ ] [[X-0003|new]]\n"
        );
    }

    #[test]
    fn add_section_block_future_appends_at_end() {
        let content = "- [ ] [[X-0001|a]]\n";
        assert_eq!(
            add_section_block(content, "- [ ] [[X-0003|new]]\n", Section::Future),
            "- [ ] [[X-0001|a]]\n\n## Future\n\n- [ ] [[X-0003|new]]\n"
        );
    }

    #[test]
    fn add_section_block_future_appends_into_legacy_futuro_header() {
        let content = "- [ ] [[X-0001|a]]\n\n## Futuro\n- [ ] [[X-0002|f]]\n";
        let got = add_section_block(content, "- [ ] [[X-0003|new]]\n", Section::Future);
        assert!(got.contains("## Futuro"), "legacy header preserved: {got}");
        assert!(got.contains("[[X-0003|new]]"));
        assert_eq!(got.matches("## Futuro").count(), 1);
    }

    #[test]
    fn add_section_block_lowprio_before_future() {
        let content = "- [ ] [[X-0001|a]]\n\n## Future\n- [ ] [[X-0002|f]]\n";
        assert_eq!(
            add_section_block(content, "- [ ] [[X-0003|new]]\n", Section::LowPrio),
            "- [ ] [[X-0001|a]]\n\n## Low-prio\n\n- [ ] [[X-0003|new]]\n## Future\n- [ ] [[X-0002|f]]\n"
        );
    }

    #[test]
    fn add_link_lands_above_notes_section_when_no_tasks() {
        let content = "# pwf\n\n### Notes\n- [[PWF-NOTE-0001]]\n";
        assert_eq!(
            add_link_to_index(content, NEW),
            "# pwf\n\n- [ ] [[GLP-0003|new task]]\n\n### Notes\n- [[PWF-NOTE-0001]]\n"
        );
    }

    #[test]
    fn add_link_preserves_trailing_notes_section_with_tasks_present() {
        let content = "- [ ] [[GLP-0001|first]]\n\n### Notes\n- [[PWF-NOTE-0001]]\n";
        assert_eq!(
            add_link_to_index(content, NEW),
            "- [ ] [[GLP-0003|new task]]\n- [ ] [[GLP-0001|first]]\n\n### Notes\n- [[PWF-NOTE-0001]]\n"
        );
    }
}
