use super::{markdown_line, task_link};

const NOTES_HEADER: &str = "### Notes";

pub(super) fn find_section_index(content: &str, headers: &[&str]) -> Option<usize> {
    headers.iter().find_map(|header| {
        markdown_line::lines(content).find_map(|line| {
            line.text
                .trim_end()
                .eq_ignore_ascii_case(header)
                .then_some(line.start)
        })
    })
}

pub(super) fn remove_index_link(content: &str, id: &str) -> String {
    let mut output = String::with_capacity(content.len());
    let mut preceding_whitespace = String::new();
    for line in markdown_line::lines(content) {
        let raw = &content[line.start..line.end];
        if line.text.trim().is_empty() {
            preceding_whitespace.push_str(raw);
        } else if task_link::parse(line.text).is_some_and(|link| link.id == id) {
            preceding_whitespace.clear();
        } else {
            output.push_str(&preceding_whitespace);
            preceding_whitespace.clear();
            output.push_str(raw);
        }
    }
    output.push_str(&preceding_whitespace);
    output
}

pub(super) fn add_note_link(content: &str, id: &str) -> String {
    let link = format!("- [[{id}]]");
    if let Some(index) = find_section_index(content, &[NOTES_HEADER]) {
        let after_header = line_end_after(content, index);
        let prefix = &content[..after_header];
        let suffix = &content[after_header..];
        return format!("{prefix}{link}\n{suffix}");
    }
    let prefix = content.trim_end();
    if prefix.is_empty() {
        format!("{NOTES_HEADER}\n\n{link}\n")
    } else {
        format!("{prefix}\n\n{NOTES_HEADER}\n\n{link}\n")
    }
}

pub(super) fn remove_note_link(content: &str, id: &str) -> String {
    remove_index_link(content, id)
}

pub(super) fn add_link_to_index(content: &str, link: &str) -> String {
    let block = format!("{link}\n");
    let normal_end = [
        find_line(content, |line| {
            line.strip_prefix("##")
                .and_then(|suffix| suffix.chars().next())
                .is_some_and(char::is_whitespace)
        }),
        find_line(content, |line| {
            line.strip_prefix("###").is_some_and(|suffix| {
                suffix.chars().next().is_some_and(char::is_whitespace) && suffix.trim() == "Notes"
            })
        }),
    ]
    .into_iter()
    .flatten()
    .min()
    .unwrap_or(content.len());
    let region = &content[..normal_end];
    let insert_at = find_line(region, |line| line.starts_with("- [["))
        .or_else(|| find_line(region, |line| line.starts_with("- [")))
        .unwrap_or(normal_end);

    let prefix = content[..insert_at].trim_end();
    let suffix = &content[insert_at..];
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

fn find_line(content: &str, predicate: impl Fn(&str) -> bool) -> Option<usize> {
    markdown_line::find(content, 0, |line| {
        predicate(line.strip_suffix('\r').unwrap_or(line))
    })
    .map(|line| line.start)
}

fn line_end_after(content: &str, idx: usize) -> usize {
    markdown_line::find(content, idx, |_| true).map_or(content.len(), |line| line.end)
}

#[cfg(test)]
mod tests {
    use super::{add_note_link, find_section_index, remove_index_link, remove_note_link};

    #[test]
    fn find_section_index_is_case_insensitive() {
        let content = "- [ ] [[X-0001|a]]\n\n## waiting on api\n- [ ] [[X-0002|f]]\n";
        assert!(find_section_index(content, &["## Waiting on API"]).is_some());
    }

    #[test]
    fn remove_index_link_handles_checkbox_prefix() {
        let content = "# proj\n\n- [ ] [[FOO-0001|tray gui]]\n\n## Later\n";
        assert_eq!(
            remove_index_link(content, "FOO-0001"),
            "# proj\n\n## Later\n"
        );
    }

    #[test]
    fn remove_index_link_strips_bare_wikilink_note_line() {
        let content = "# proj\n\n### Notes\n- [[FOO-NOTE-0001]]\n- [[FOO-NOTE-0002]]\n";
        assert_eq!(
            remove_index_link(content, "FOO-NOTE-0001"),
            "# proj\n\n### Notes\n- [[FOO-NOTE-0002]]\n"
        );
    }

    #[test]
    fn note_link_creates_final_section_without_clobbering_tasks() {
        let content = "- [ ] [[FOO-0001|task]]\n\n## Future\n- [ ] [[FOO-0002|later]]\n";
        assert_eq!(
            add_note_link(content, "FOO-NOTE-0001"),
            "- [ ] [[FOO-0001|task]]\n\n## Future\n- [ ] [[FOO-0002|later]]\n\n### Notes\n\n- [[FOO-NOTE-0001]]\n"
        );
    }

    #[test]
    fn note_link_creates_section_in_empty_index() {
        assert_eq!(
            add_note_link("", "FOO-NOTE-0001"),
            "### Notes\n\n- [[FOO-NOTE-0001]]\n"
        );
    }

    #[test]
    fn note_link_inserts_newest_first_under_existing_header() {
        let content = "# foo\n\n### Notes\n- [[FOO-NOTE-0001]]\n";
        let updated = add_note_link(content, "FOO-NOTE-0002");
        assert_eq!(
            updated,
            "# foo\n\n### Notes\n- [[FOO-NOTE-0002]]\n- [[FOO-NOTE-0001]]\n"
        );
        assert_eq!(updated.matches("### Notes").count(), 1);
    }

    #[test]
    fn note_link_removal_strips_only_the_target() {
        let content = "### Notes\n- [[FOO-NOTE-0001]]\n- [[FOO-NOTE-0002]]\n";
        assert_eq!(
            remove_note_link(content, "FOO-NOTE-0001"),
            "### Notes\n- [[FOO-NOTE-0002]]\n"
        );
    }
}
