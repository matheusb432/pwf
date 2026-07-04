//! Pure title-formatting helpers: whitespace collapsing and word-boundary-safe capping.

/// Collapses internal whitespace runs to single spaces and trims the ends.
pub(crate) fn single_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Caps `title` at `max_chars`, breaking on a word boundary and appending an
/// ellipsis when truncated. Falls back to a char-boundary cut when a single
/// word exceeds `max_chars` on its own (e.g. a URL with no spaces).
pub(crate) fn cap_title(title: &str, max_chars: usize) -> String {
    if title.chars().count() <= max_chars {
        return title.to_string();
    }
    let mut out = String::new();
    for word in title.split(' ') {
        let with_word = out.chars().count() + usize::from(!out.is_empty()) + word.chars().count();
        if with_word > max_chars {
            break;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    if out.is_empty() {
        out = title.chars().take(max_chars).collect();
    }
    format!("{out}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_text_unchanged_when_within_cap() {
        assert_eq!(cap_title("short title", 80), "short title");
    }

    #[test]
    fn caps_at_word_boundary_with_ellipsis_when_over_limit() {
        let title = "word ".repeat(20);
        let capped = cap_title(title.trim(), 10);
        assert!(capped.chars().count() <= 11);
        assert!(capped.ends_with('…'));
        assert!(!capped.contains("word word word"));
    }

    #[test]
    fn caps_single_overlong_word_on_char_boundary() {
        let word = "x".repeat(200);
        let capped = cap_title(&word, 80);
        assert!(capped.ends_with('…'));
        assert_eq!(capped.chars().count(), 81);
    }

    #[test]
    fn single_line_collapses_internal_whitespace_and_trims() {
        assert_eq!(single_line("  a   b\nc  "), "a b c");
        assert_eq!(single_line(""), "");
    }
}
