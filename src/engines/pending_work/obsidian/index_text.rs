// Project-index string transforms.

use super::super::section::Section;
use regex::Regex;

/// Insert `link\n` at the top of the normal-item region: before the first
/// `^- \[` line that precedes any `## ` section, else after the leading preamble
/// (H1/blockquote) but above the sections.
///
/// Anchoring on a checkbox *anywhere* in the file let a new item land inside
/// `## Future`/`## Human` when those sections held the only checkboxes; bounding
/// the search to the region before the first `## ` header keeps it in the normal
/// region. A leading H1 or blockquote is preamble; the item lands after it.
pub fn add_link_to_index(content: &str, link: &str) -> String {
    let block = format!("{link}\n");

    let section_re = Regex::new(r"(?m)^##\s").unwrap();
    let normal_end = section_re
        .find(content)
        .map(|m| m.start())
        .unwrap_or(content.len());
    let region = &content[..normal_end];

    let anchor_wikilink = Regex::new(r"(?m)^- \[\[").unwrap();
    let anchor_checkbox = Regex::new(r"(?m)^- \[").unwrap();
    let insert_at = anchor_wikilink
        .find(region)
        .or_else(|| anchor_checkbox.find(region))
        .map(|m| m.start())
        .unwrap_or(normal_end);

    let prefix = content[..insert_at].trim_end();
    let suffix = &content[insert_at..];
    // block ends with '\n'; leave a blank line before a following heading, but stay
    // adjacent to a following list item.
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

/// Remove the line matching `[[<id>|...]]` or `[[<id>]]`, with or without a leading
/// `- [ ] ` Obsidian checkbox.
pub fn remove_index_link(content: &str, id: &str) -> String {
    let pattern = format!(
        r"(?m)^\s*-\s*(?:\[ \]\s*)?\[\[{}(?:\|[^\]]*)?\]\].*(?:\r?\n)?",
        regex::escape(id)
    );
    let re = Regex::new(&pattern).unwrap();
    re.replace_all(content, "").into_owned()
}

/// Return byte offset of the first matching header, or `None`. Accepts headers in
/// preference order. Matching is case-insensitive (PWF-0026).
pub fn find_section_index(content: &str, headers: &[&str]) -> Option<usize> {
    for h in headers {
        let pattern = format!(r"(?im)^{}\s*$", regex::escape(h));
        if let Some(m) = Regex::new(&pattern).unwrap().find(content) {
            return Some(m.start());
        }
    }
    None
}

/// True if `section`'s header already exists in `content` (case-insensitive).
pub fn section_exists(content: &str, section: Section) -> bool {
    find_section_index(content, section.read_headers()).is_some()
}

fn line_end_after(content: &str, idx: usize) -> usize {
    content[idx..]
        .find('\n')
        .map(|i| idx + i + 1)
        .unwrap_or(content.len())
}

fn skip_blank_lines(content: &str, mut idx: usize) -> usize {
    while idx < content.len() {
        let rest = &content[idx..];
        let line_len = rest.find('\n').map(|i| i + 1).unwrap_or(rest.len());
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

/// Insert `block` into the `section` ('Future', 'Human', or 'Low-prio'), creating the
/// header if needed. Section order: normal -> Low-prio -> Human -> Future (last).
/// A legacy `## Futuro` header is still recognized, but new sections are created
/// as `## Future` (PWF-0026).
pub fn add_section_block(content: &str, block: &str, section: Section) -> String {
    let headers = section.read_headers();
    let section_idx = find_section_index(content, headers);

    if let Some(idx) = section_idx {
        return insert_section_item(content, line_end_after(content, idx), block);
    }

    // Section does not exist; create it in the correct position.
    let header = format!("## {}", section.as_str());

    if section == Section::Future {
        // Append at the very end.
        let prefix = content.trim_end();
        return format!("{prefix}\n\n{header}\n\n{block}");
    }

    // Low-prio / Human: place immediately before Future (incl. legacy Futuro) if it
    // exists, else at end.
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
        // PWF-0006/1: when the only checkboxes live under `## Future`, the new item
        // must go into the (empty) normal region above the heading, never inside it.
        let content = "## Future\n- [ ] [[GLP-0007|later]]\n";
        assert_eq!(
            add_link_to_index(content, NEW),
            "- [ ] [[GLP-0003|new task]]\n\n## Future\n- [ ] [[GLP-0007|later]]\n"
        );
    }

    #[test]
    fn add_link_below_leading_h1() {
        // Preserve migrated behavior: item lands below the H1 title.
        assert_eq!(
            add_link_to_index("# glep-shimeji\n", NEW),
            "# glep-shimeji\n\n- [ ] [[GLP-0003|new task]]\n"
        );
    }

    #[test]
    fn add_link_after_leading_blockquote() {
        // A leading Obsidian callout is preamble: the item lands after the blockquote.
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
        // PWF-0006/6: a second Human add must land within the existing `## Human`
        // section, not create a duplicate header.
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
        // New Future sections are created as `## Future` (never `## Futuro`).
        let content = "- [ ] [[X-0001|a]]\n";
        assert_eq!(
            add_section_block(content, "- [ ] [[X-0003|new]]\n", Section::Future),
            "- [ ] [[X-0001|a]]\n\n## Future\n\n- [ ] [[X-0003|new]]\n"
        );
    }

    #[test]
    fn add_section_block_future_appends_into_legacy_futuro_header() {
        // A pre-existing `## Futuro` is still recognized for insertion.
        let content = "- [ ] [[X-0001|a]]\n\n## Futuro\n- [ ] [[X-0002|f]]\n";
        let got = add_section_block(content, "- [ ] [[X-0003|new]]\n", Section::Future);
        assert!(got.contains("## Futuro"), "legacy header preserved: {got}");
        assert!(got.contains("[[X-0003|new]]"));
        assert_eq!(got.matches("## Futuro").count(), 1);
    }

    #[test]
    fn find_section_index_is_case_insensitive() {
        let content = "- [ ] [[X-0001|a]]\n\n## low-prio\n- [ ] [[X-0002|f]]\n";
        assert!(find_section_index(content, &["## Low-prio"]).is_some());
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
    fn remove_index_link_handles_checkbox_prefix() {
        // pwf/Obsidian items use the checkbox form "- [ ] [[ID|title]]"; check must
        // still strip them, exactly as it does the bare "- [[ID|title]]" form.
        let content = "# proj\n\n- [ ] [[GLP-0001|tray gui]]\n\n## Later\n";
        assert_eq!(
            remove_index_link(content, "GLP-0001"),
            "# proj\n\n## Later\n"
        );
    }
}
