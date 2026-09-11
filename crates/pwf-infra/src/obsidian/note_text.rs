use super::markdown_line;

/// Replaces the body while preserving frontmatter.
///
/// A note without a closing fence is treated as body-only.
pub(super) fn replace_body(content: &str, body: &str) -> String {
    let mut fences = markdown_line::lines(content)
        .filter(|line| is_frontmatter_fence(line.text))
        .map(|line| line.content_end);
    if fences.next().is_some()
        && let Some(close_end) = fences.next()
    {
        return format!("{}\n\n{}\n", &content[..close_end], body.trim_end());
    }
    format!("{}\n", body.trim_end())
}

fn is_frontmatter_fence(line: &str) -> bool {
    line.strip_prefix("---").is_some_and(|suffix| {
        suffix
            .chars()
            .all(|character| matches!(character, ' ' | '\t'))
    })
}

#[cfg(test)]
mod tests {
    use super::replace_body;

    #[test]
    fn body_replacement_preserves_frontmatter() {
        let source = "---\nid: FOO-0001\nstatus: active\ntitle: old\n---\n\nold body\n";

        assert_eq!(
            replace_body(source, "new body"),
            "---\nid: FOO-0001\nstatus: active\ntitle: old\n---\n\nnew body\n"
        );
    }

    #[test]
    fn body_only_source_stays_body_only() {
        assert_eq!(replace_body("old body\n", "new body\n\n"), "new body\n");
    }
}
