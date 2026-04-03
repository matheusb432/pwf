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

/// Builds the explicit `--review` task prompt for `reviewed_id`, `&`-segmented so
/// `text::goals_body` renders one bullet per command (and the lead segment infers the
/// title). pwf only *emits* these git-tools command strings — it never shells out.
///
/// With a `range` the diff is scoped to it; without one it falls back to git-tools'
/// unpushed default. The literal command strings live only here (no duplication).
pub(super) fn review_task_prompt(reviewed_id: &str, range: Option<&str>) -> String {
    let diff = match range {
        Some(r) => format!("git-tools diff {r}"),
        None => "git-tools diff".to_string(),
    };
    format!("review {reviewed_id} & {diff} & git-tools diff-subrepos")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| s.to_string()).collect()
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

    #[test]
    fn review_prompt_with_range_targets_the_range() {
        let p = review_task_prompt("GLP-0001", Some("a..b"));
        assert_eq!(
            p,
            "review GLP-0001 & git-tools diff a..b & git-tools diff-subrepos"
        );
    }

    #[test]
    fn review_prompt_without_range_falls_back_to_bare_diff() {
        let p = review_task_prompt("GLP-0001", None);
        assert_eq!(
            p,
            "review GLP-0001 & git-tools diff & git-tools diff-subrepos"
        );
        // No range token leaks into the fallback.
        assert!(!p.contains(".."), "fallback must not carry a range: {p}");
    }
}
