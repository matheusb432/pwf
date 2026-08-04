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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum KnownSection {
    Future,
    Human,
    LowPrio,
}

impl KnownSection {
    pub(super) fn parse(value: &str) -> Option<Self> {
        match value {
            "Future" => Some(Self::Future),
            "Human" => Some(Self::Human),
            "Low-prio" => Some(Self::LowPrio),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Future => "Future",
            Self::Human => "Human",
            Self::LowPrio => "Low-prio",
        }
    }

    fn read_headers(self) -> &'static [&'static str] {
        match self {
            Self::Future => &["## Future", "## Futuro"],
            Self::Human => &["## Human"],
            Self::LowPrio => &["## Low-prio", "## Low-priority"],
        }
    }
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

pub(super) fn section_exists(content: &str, section: KnownSection) -> bool {
    find_section_index(content, section.read_headers()).is_some()
}

fn line_end_after(content: &str, idx: usize) -> usize {
    markdown_line::find(content, idx, |_| true).map_or(content.len(), |line| line.end)
}

fn skip_blank_lines(content: &str, idx: usize) -> usize {
    markdown_line::find(content, idx, |line| !line.trim().is_empty())
        .map_or(content.len(), |line| line.start)
}

fn insert_section_task(content: &str, insert_at: usize, block: &str) -> String {
    let suffix_at = skip_blank_lines(content, insert_at);
    let prefix = content[..insert_at].trim_end();
    let suffix = &content[suffix_at..];
    if suffix.is_empty() {
        format!("{prefix}\n{block}")
    } else {
        format!("{prefix}\n{block}\n{suffix}")
    }
}

pub(super) fn add_section_block(content: &str, block: &str, section: KnownSection) -> String {
    if let Some(idx) = find_section_index(content, section.read_headers()) {
        return insert_section_task(content, line_end_after(content, idx), block);
    }

    let header = format!("## {}", section.as_str());
    if section == KnownSection::Future {
        let prefix = content.trim_end();
        return format!("{prefix}\n\n{header}\n\n{block}");
    }

    if let Some(future_idx) = find_section_index(content, KnownSection::Future.read_headers()) {
        let prefix = content[..future_idx].trim_end();
        let suffix = &content[future_idx..];
        return format!("{prefix}\n\n{header}\n\n{block}{suffix}");
    }

    let prefix = content.trim_end();
    format!("{prefix}\n\n{header}\n\n{block}")
}

#[cfg(test)]
mod tests {
    use super::{add_note_link, find_section_index, remove_index_link, remove_note_link};

    #[test]
    fn find_section_index_is_case_insensitive() {
        let content = "- [ ] [[X-0001|a]]\n\n## low-prio\n- [ ] [[X-0002|f]]\n";
        assert!(find_section_index(content, &["## Low-prio"]).is_some());
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
        let content = "# proj\n\n### Notes\n- [[PWF-NOTE-0001]]\n- [[PWF-NOTE-0002]]\n";
        assert_eq!(
            remove_index_link(content, "PWF-NOTE-0001"),
            "# proj\n\n### Notes\n- [[PWF-NOTE-0002]]\n"
        );
    }

    #[test]
    fn note_link_creates_final_section_without_clobbering_tasks() {
        let content = "- [ ] [[PWF-0001|task]]\n\n## Future\n- [ ] [[PWF-0002|later]]\n";
        assert_eq!(
            add_note_link(content, "PWF-NOTE-0001"),
            "- [ ] [[PWF-0001|task]]\n\n## Future\n- [ ] [[PWF-0002|later]]\n\n### Notes\n\n- [[PWF-NOTE-0001]]\n"
        );
    }

    #[test]
    fn note_link_creates_section_in_empty_index() {
        assert_eq!(
            add_note_link("", "PWF-NOTE-0001"),
            "### Notes\n\n- [[PWF-NOTE-0001]]\n"
        );
    }

    #[test]
    fn note_link_inserts_newest_first_under_existing_header() {
        let content = "# pwf\n\n### Notes\n- [[PWF-NOTE-0001]]\n";
        let updated = add_note_link(content, "PWF-NOTE-0002");
        assert_eq!(
            updated,
            "# pwf\n\n### Notes\n- [[PWF-NOTE-0002]]\n- [[PWF-NOTE-0001]]\n"
        );
        assert_eq!(updated.matches("### Notes").count(), 1);
    }

    #[test]
    fn note_link_removal_strips_only_the_target() {
        let content = "### Notes\n- [[PWF-NOTE-0001]]\n- [[PWF-NOTE-0002]]\n";
        assert_eq!(
            remove_note_link(content, "PWF-NOTE-0001"),
            "### Notes\n- [[PWF-NOTE-0002]]\n"
        );
    }
}
