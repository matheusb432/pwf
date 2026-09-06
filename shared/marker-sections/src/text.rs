//! Single-line whitespace normalization.

pub(crate) fn single_line(text: &str) -> String {
    let mut words = text.split_whitespace();
    let Some(first) = words.next() else {
        return String::new();
    };
    let mut normalized = String::with_capacity(text.len());
    normalized.push_str(first);
    for word in words {
        normalized.push(' ');
        normalized.push_str(word);
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::single_line;

    #[test]
    fn single_line_collapses_whitespace_without_a_word_buffer() {
        assert_eq!(single_line("  alpha\n beta\t gamma  "), "alpha beta gamma");
        assert_eq!(single_line(" \n\t "), "");
    }
}
