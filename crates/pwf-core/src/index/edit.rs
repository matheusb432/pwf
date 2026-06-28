//! Pure, domain-agnostic transforms over a project index's Markdown text.

use regex::Regex;

/// Return byte offset of the first matching header, or `None`. Accepts headers
/// in preference order. Matching is case-insensitive (PWF-0026).
pub fn find_section_index(content: &str, headers: &[&str]) -> Option<usize> {
    for h in headers {
        let pattern = format!(r"(?im)^{}\s*$", regex::escape(h));
        if let Some(m) = Regex::new(&pattern).unwrap().find(content) {
            return Some(m.start());
        }
    }
    None
}

/// Remove the line matching `[[<id>|...]]` or `[[<id>]]`, with or without a
/// leading `- [ ] ` Obsidian checkbox.
pub fn remove_index_link(content: &str, id: &str) -> String {
    let pattern = format!(
        r"(?m)^\s*-\s*(?:\[ \]\s*)?\[\[{}(?:\|[^\]]*)?\]\].*(?:\r?\n)?",
        regex::escape(id)
    );
    let re = Regex::new(&pattern).unwrap();
    re.replace_all(content, "").into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_section_index_is_case_insensitive() {
        let content = "- [ ] [[X-0001|a]]\n\n## low-prio\n- [ ] [[X-0002|f]]\n";
        assert!(find_section_index(content, &["## Low-prio"]).is_some());
    }

    #[test]
    fn remove_index_link_handles_checkbox_prefix() {
        let content = "# proj\n\n- [ ] [[GLP-0001|tray gui]]\n\n## Later\n";
        assert_eq!(
            remove_index_link(content, "GLP-0001"),
            "# proj\n\n## Later\n"
        );
    }

    #[test]
    fn remove_index_link_strips_bare_wikilink_note_line() {
        // A note line is a bare `- [[ID]]` (no checkbox); removal must strip it.
        let content = "# proj\n\n### Notes\n- [[PWF-NOTE-0001]]\n- [[PWF-NOTE-0002]]\n";
        assert_eq!(
            remove_index_link(content, "PWF-NOTE-0001"),
            "# proj\n\n### Notes\n- [[PWF-NOTE-0002]]\n"
        );
    }
}
