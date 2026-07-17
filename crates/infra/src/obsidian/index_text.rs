use std::sync::LazyLock;

pub use pwf_core::index::edit::{find_section_index, remove_index_link};
use regex::Regex;

static SECTION_MARK_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^##\s").unwrap());
static NOTES_HEADER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^###\s+Notes\s*$").unwrap());
static ANCHOR_WIKILINK_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^- \[\[").unwrap());
static ANCHOR_CHECKBOX_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^- \[").unwrap());

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

pub(super) fn section_exists(content: &str, section: KnownSection) -> bool {
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

pub(super) fn add_section_block(content: &str, block: &str, section: KnownSection) -> String {
    if let Some(idx) = find_section_index(content, section.read_headers()) {
        return insert_section_item(content, line_end_after(content, idx), block);
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
