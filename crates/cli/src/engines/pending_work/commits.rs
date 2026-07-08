// Commit-range provenance recorded on close-out (PWF-0017). Owns the `commits:`
// frontmatter value format and the explicit `--review` task prompt. A range is
// provenance, not a pwf id — no `[[…]]` wrapping, no config validation here.

/// Normalizes the repeated/comma-separated `--commits` values into a single
/// frontmatter value: split each on `,`, trim, drop empties, dedup (preserving
/// first-seen order), join with `", "`. `None` when nothing usable remains.
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
