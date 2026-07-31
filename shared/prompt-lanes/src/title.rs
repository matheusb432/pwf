//! Whitespace normalization for prompt titles and bullets.

/// Collapses whitespace runs and trims both ends.
pub(crate) fn single_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
