// Commit ranges are opaque provenance, not pending-work IDs.

/// Normalizes repeated or comma-separated ranges, preserving first-seen order and removing
/// duplicates. Returns `None` when no non-empty range remains.
pub(super) fn frontmatter_value(values: &[String]) -> Option<String> {
    let mut ranges: Vec<String> = Vec::new();
    for value in values {
        for raw in value.split(',') {
            let raw = raw.trim();
            if raw.is_empty() {
                continue;
            }
            if !ranges.iter().any(|r| r == raw) {
                ranges.push(raw.to_string());
            }
        }
    }
    (!ranges.is_empty()).then(|| ranges.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(parts: &[&str]) -> Vec<String> {
        parts.iter().map(std::string::ToString::to_string).collect()
    }

    #[test]
    fn empty_input_is_none() {
        assert_eq!(frontmatter_value(&[]), None);
        assert_eq!(frontmatter_value(&v(&["", "  ", ","])), None);
    }

    #[test]
    fn single_range_passes_through_raw() {
        assert_eq!(
            frontmatter_value(&v(&["a1b2c3d..f4e5d6c"])),
            Some("a1b2c3d..f4e5d6c".to_string())
        );
    }

    #[test]
    fn repeated_and_comma_forms_join_and_dedup() {
        assert_eq!(
            frontmatter_value(&v(&["a..b", "c..d"])),
            Some("a..b, c..d".to_string())
        );
        assert_eq!(
            frontmatter_value(&v(&["a..b,c..d"])),
            Some("a..b, c..d".to_string())
        );
        assert_eq!(
            frontmatter_value(&v(&["a..b", "a..b", "c..d"])),
            Some("a..b, c..d".to_string())
        );
    }
}
