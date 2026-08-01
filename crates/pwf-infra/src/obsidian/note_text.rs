use std::sync::LazyLock;

use regex::Regex;

static TITLE_LINE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^title:.*$").unwrap());
static FRONTMATTER_FENCE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^---[ \t]*\r?$").unwrap());

/// Replaces the raw `title:` frontmatter line.
pub(super) fn replace_title(content: &str, title: &str) -> String {
    TITLE_LINE_RE
        .replace(content, format!("title: {title}").as_str())
        .into_owned()
}

/// Replaces the body while preserving frontmatter.
///
/// A note without a closing fence is treated as body-only.
pub(super) fn replace_body(content: &str, body: &str) -> String {
    let mut fences = FRONTMATTER_FENCE_RE.find_iter(content);
    match (fences.next(), fences.next()) {
        (Some(_), Some(close)) => {
            format!("{}\n\n{}\n", &content[..close.end()], body.trim_end())
        }
        _ => format!("{}\n", body.trim_end()),
    }
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
