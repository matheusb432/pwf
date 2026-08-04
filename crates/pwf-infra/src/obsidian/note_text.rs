/// Replaces the raw `title:` frontmatter line.
pub(super) fn replace_title(content: &str, title: &str) -> String {
    let Some((start, end)) = find_line(content, |line| line.starts_with("title:")) else {
        return content.to_string();
    };
    format!("{}title: {title}{}", &content[..start], &content[end..])
}

/// Replaces the body while preserving frontmatter.
///
/// A note without a closing fence is treated as body-only.
pub(super) fn replace_body(content: &str, body: &str) -> String {
    let mut fences = line_ranges(content)
        .filter(|(_, _, line)| is_frontmatter_fence(line))
        .map(|(_, end, _)| end);
    if fences.next().is_some()
        && let Some(close_end) = fences.next()
    {
        return format!("{}\n\n{}\n", &content[..close_end], body.trim_end());
    }
    format!("{}\n", body.trim_end())
}

fn find_line(content: &str, predicate: impl Fn(&str) -> bool) -> Option<(usize, usize)> {
    line_ranges(content).find_map(|(start, end, line)| predicate(line).then_some((start, end)))
}

fn line_ranges(content: &str) -> impl Iterator<Item = (usize, usize, &str)> {
    let mut index = 0;
    content.split_inclusive('\n').map(move |line| {
        let start = index;
        index += line.len();
        let without_newline = line.strip_suffix('\n').unwrap_or(line);
        (start, start + without_newline.len(), without_newline)
    })
}

fn is_frontmatter_fence(line: &str) -> bool {
    line.strip_suffix('\r')
        .unwrap_or(line)
        .strip_prefix("---")
        .is_some_and(|suffix| {
            suffix
                .chars()
                .all(|character| matches!(character, ' ' | '\t'))
        })
}

#[cfg(test)]
mod tests {
    use super::{replace_body, replace_title};

    #[test]
    fn replacements_preserve_the_unedited_note_region() {
        let source = "---\nid: PWF-0001\nstatus: active\ntitle: old\n---\n\nold body\n";
        let titled = replace_title(source, "new title");

        assert_eq!(
            replace_body(&titled, "new body"),
            "---\nid: PWF-0001\nstatus: active\ntitle: new title\n---\n\nnew body\n"
        );
    }

    #[test]
    fn body_only_source_stays_body_only() {
        assert_eq!(replace_body("old body\n", "new body\n\n"), "new body\n");
    }
}
