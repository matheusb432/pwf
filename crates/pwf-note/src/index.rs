//! Maintains the final `### Notes` section of a project index.
//!
//! New links are inserted newest-first directly under the header.

use pwf_core::index::edit::{find_section_index, remove_index_link};

const NOTES_HEADER: &str = "### Notes";

/// Inserts `- [[id]]` newest-first under a final `### Notes` section.
///
/// A missing section is appended after trimming trailing whitespace.
pub fn add_note_link(content: &str, id: &str) -> String {
    let line = format!("- [[{id}]]");
    if let Some(idx) = find_section_index(content, &[NOTES_HEADER]) {
        let after_header = content[idx..]
            .find('\n')
            .map_or(content.len(), |i| idx + i + 1);
        let prefix = &content[..after_header];
        let suffix = &content[after_header..];
        format!("{prefix}{line}\n{suffix}")
    } else {
        let prefix = content.trim_end();
        if prefix.is_empty() {
            format!("{NOTES_HEADER}\n\n{line}\n")
        } else {
            format!("{prefix}\n\n{NOTES_HEADER}\n\n{line}\n")
        }
    }
}

/// Removes the `- [[id]]` note line.
pub fn remove_note_link(content: &str, id: &str) -> String {
    remove_index_link(content, id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_section_at_end_when_absent() {
        let content = "- [ ] [[PWF-0001|task]]\n\n## Future\n- [ ] [[PWF-0002|later]]\n";
        let got = add_note_link(content, "PWF-NOTE-0001");
        assert_eq!(
            got,
            "- [ ] [[PWF-0001|task]]\n\n## Future\n- [ ] [[PWF-0002|later]]\n\n### Notes\n\n- [[PWF-NOTE-0001]]\n"
        );
    }

    #[test]
    fn creates_section_into_empty_index() {
        assert_eq!(
            add_note_link("", "PWF-NOTE-0001"),
            "### Notes\n\n- [[PWF-NOTE-0001]]\n"
        );
    }

    #[test]
    fn inserts_newest_first_under_existing_header() {
        let content = "# pwf\n\n### Notes\n- [[PWF-NOTE-0001]]\n";
        let got = add_note_link(content, "PWF-NOTE-0002");
        assert_eq!(
            got,
            "# pwf\n\n### Notes\n- [[PWF-NOTE-0002]]\n- [[PWF-NOTE-0001]]\n"
        );
        assert_eq!(got.matches("### Notes").count(), 1);
    }

    #[test]
    fn remove_strips_only_the_targeted_note() {
        let content = "### Notes\n- [[PWF-NOTE-0001]]\n- [[PWF-NOTE-0002]]\n";
        assert_eq!(
            remove_note_link(content, "PWF-NOTE-0001"),
            "### Notes\n- [[PWF-NOTE-0002]]\n"
        );
    }
}
