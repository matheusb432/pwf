//! Whitespace normalization for prompt titles and bullets.

/// Collapses whitespace runs and trims both ends.
pub(crate) fn single_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_line_collapses_internal_whitespace_and_trims() {
        assert_eq!(single_line("  a   b\nc  "), "a b c");
        assert_eq!(single_line(""), "");
    }
}
