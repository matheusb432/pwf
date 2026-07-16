use std::sync::LazyLock;

use regex::Regex;

static TITLE_LINE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^title:.*$").unwrap());
static FRONTMATTER_FENCE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^---[ \t]*\r?$").unwrap());

/// Replaces the `title:` frontmatter line — a representation edit over the raw
/// note text (unlike the pure body transforms in `pwf_domain`'s `note_body`).
pub(super) fn replace_title(content: &str, title: &str) -> String {
    TITLE_LINE_RE
        .replace(content, format!("title: {title}").as_str())
        .into_owned()
}

/// Splices a new body between the frontmatter fences, keeping the frontmatter
/// verbatim. A note with no closing fence is treated as body-only.
pub(super) fn replace_body(content: &str, body: &str) -> String {
    let mut fences = FRONTMATTER_FENCE_RE.find_iter(content);
    match (fences.next(), fences.next()) {
        (Some(_), Some(close)) => {
            format!("{}\n\n{}\n", &content[..close.end()], body.trim_end())
        }
        _ => format!("{}\n", body.trim_end()),
    }
}
