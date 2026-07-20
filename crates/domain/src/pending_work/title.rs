use nutype::nutype;
use prompt_lanes::parse;

const DEFAULT_TASK_TITLE: &str = "n/a";
const MAX_TITLE_CHARS: usize = 80;

/// YAML indicator characters that cannot open an unquoted plain scalar.
const YAML_UNSAFE_LEADING_CHARS: [char; 16] = [
    ',', '[', ']', '{', '}', '#', '&', '*', '!', '|', '>', '\'', '"', '%', '@', '`',
];

#[nutype(
    sanitize(trim, with = |raw: String| normalize_task_title(&raw)),
    validate(not_empty),
    derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, AsRef, Display),
)]
pub struct TaskTitle(String);

impl Default for TaskTitle {
    fn default() -> Self {
        Self::try_new(DEFAULT_TASK_TITLE).expect("default task title is valid")
    }
}

fn normalize_task_title(title: &str) -> String {
    title.to_lowercase()
}

/// Lowercases a title and rewrites it into a single-line YAML-safe plain scalar.
///
/// Titles are stored as unquoted `title: <value>` frontmatter lines, so mapping-breaking input is
/// replaced or dropped. The validated default title is returned when nothing displayable survives.
///
/// # Examples
///
/// ```
/// use pwf_domain::pending_work::normalize_title;
///
/// assert_eq!(
///     normalize_title("Fix parser: preserve YAML"),
///     "fix parser; preserve yaml"
/// );
/// ```
#[must_use]
pub fn normalize_title(title: &str) -> String {
    let safe = yaml_plain_scalar(&title.to_lowercase());
    if safe.is_empty() {
        TaskTitle::default().to_string()
    } else {
        safe
    }
}

/// Reports whether [`normalize_title`] changed `raw` beyond trimming and lowercasing.
///
/// # Examples
///
/// ```
/// use pwf_domain::pending_work::title_was_normalized;
///
/// assert!(title_was_normalized("fix parser: preserve YAML"));
/// assert!(!title_was_normalized("  Fix Parser  "));
/// ```
#[must_use]
pub fn title_was_normalized(raw: &str) -> bool {
    normalize_title(raw) != raw.trim().to_lowercase()
}

fn yaml_plain_scalar(title: &str) -> String {
    let collapsed = title.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out = String::with_capacity(collapsed.len());
    let mut chars = collapsed.chars().peekable();
    let mut opens_comment = true;
    while let Some(c) = chars.next() {
        match c {
            ':' => {
                let mut run_length = 1;
                while chars.next_if_eq(&':').is_some() {
                    run_length += 1;
                }
                match chars.peek() {
                    None => {}
                    Some(' ') => out.push(';'),
                    Some(_) => out.extend(std::iter::repeat_n(':', run_length)),
                }
                opens_comment = false;
            }
            '#' if opens_comment => {}
            _ => {
                out.push(c);
                opens_comment = c == ' ';
            }
        }
    }
    strip_unsafe_leading_chars(out.trim_end()).to_string()
}

fn strip_unsafe_leading_chars(mut value: &str) -> &str {
    loop {
        value = value.trim_start_matches(' ');
        let mut chars = value.chars();
        let Some(first) = chars.next() else {
            return value;
        };
        let unsafe_lead = YAML_UNSAFE_LEADING_CHARS.contains(&first)
            || (matches!(first, '-' | '?' | ':')
                && chars.next().is_none_or(|second| second == ' '));
        if !unsafe_lead {
            return value;
        }
        value = &value[first.len_utf8()..];
    }
}

/// Returns the capped, normalized lead clause or [`TaskTitle::default`] for a marker-first prompt.
///
/// # Examples
///
/// ```
/// use pwf_domain::pending_work::inferred_title;
///
/// assert_eq!(inferred_title("ship it / preserve output"), "ship it");
/// ```
#[must_use]
pub fn inferred_title(prompt: &str) -> String {
    normalize_title(&parse(prompt).capped_title(MAX_TITLE_CHARS))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAX_RENDERED_TITLE_CHARS: usize = 81;

    #[test]
    fn task_title_trims_lowercases_and_rejects_blank() {
        assert_eq!(
            TaskTitle::try_new("  Fix The THING  ").unwrap().as_ref(),
            "fix the thing"
        );
        assert!(TaskTitle::try_new(" \n ").is_err());
    }

    #[test]
    fn missing_task_title_defaults_to_validated_na_value() {
        let title = TaskTitle::default();

        assert_eq!(title.as_ref(), "n/a");
        assert_eq!(TaskTitle::try_new(title.to_string()).unwrap(), title);
    }

    #[test]
    fn title_inference_keeps_full_prompt_without_ampersand() {
        assert_eq!(
            inferred_title("Build the data filter to let users sort"),
            "build the data filter to let users sort"
        );
        assert_eq!(inferred_title("add startup toggle"), "add startup toggle");
    }

    #[test]
    fn title_inference_cuts_at_first_lane_marker_only() {
        assert_eq!(
            inferred_title("create engine feature to add update task / make it idempotent"),
            "create engine feature to add update task"
        );
        assert_eq!(inferred_title("a / b / c"), "a");
        assert_eq!(
            inferred_title("fix bug: empty prompt"),
            "fix bug; empty prompt"
        );
    }

    #[test]
    fn title_inference_collapses_whitespace_and_lowercases() {
        assert_eq!(
            inferred_title("  Refactor   Help  Command  "),
            "refactor help command"
        );
    }

    #[test]
    fn title_inference_empty_lead_uses_domain_fallback() {
        assert_eq!(inferred_title("/ only second"), "n/a");
        assert_eq!(inferred_title("/c context"), "n/a");
    }

    #[test]
    fn title_inference_caps_long_prompt_without_marker_at_word_boundary() {
        let prompt = "Continue the PowerShell to Rust port into the cfgtool CLI (scripts/cfgtool), using the shipped gaming domain as the template, porting domain-by-domain smallest first";
        let title = inferred_title(prompt);
        assert_eq!(
            title,
            "continue the powershell to rust port into the cfgtool cli (scripts/cfgtool),…"
        );
        assert!(title.chars().count() <= MAX_RENDERED_TITLE_CHARS);
        assert!(
            prompt
                .to_lowercase()
                .starts_with(title.trim_end_matches('…'))
        );
    }

    #[test]
    fn title_inference_caps_long_lead_clause_before_marker() {
        let prompt = "HUMAN: Start studying AZ-104 Section 02 - Storage. Begin with the Storage MOC, then cover Storage Accounts / Redundancy / Security";
        let title = inferred_title(prompt);
        assert_eq!(
            title,
            "human; start studying az-104 section 02 - storage. begin with the storage moc,…"
        );
        assert!(title.chars().count() <= MAX_RENDERED_TITLE_CHARS);
    }

    #[test]
    fn title_inference_short_prompt_passes_through_uncapped() {
        assert_eq!(
            inferred_title("add a startup toggle to the settings page"),
            "add a startup toggle to the settings page"
        );
    }

    #[test]
    fn title_inference_caps_single_overlong_word_on_char_boundary() {
        let word = "x".repeat(200);
        let title = inferred_title(&word);
        assert!(title.ends_with('…'));
        assert_eq!(title.chars().count(), MAX_RENDERED_TITLE_CHARS);
    }

    #[test]
    fn title_inference_is_always_bounded() {
        let cases = [
            "x".repeat(500),
            format!("{} / tail", "word ".repeat(200)),
            "/ ".repeat(300),
            "💥".repeat(300),
            "a ".repeat(300),
            String::new(),
        ];
        for input in cases {
            let title = inferred_title(&input);
            assert!(
                title.chars().count() <= MAX_RENDERED_TITLE_CHARS,
                "input bound violated ({} chars): {title}",
                title.chars().count()
            );
        }
    }

    #[test]
    fn normalize_title_lowercases_and_leaves_safe_titles_untouched() {
        assert_eq!(normalize_title("HUMAN: Do AZ-104"), "human; do az-104");
        assert_eq!(normalize_title("already lower"), "already lower");
        assert_eq!(normalize_title("time is 3:30pm"), "time is 3:30pm");
        assert_eq!(normalize_title("fix foo::bar panic"), "fix foo::bar panic");
        assert_eq!(
            normalize_title("read https://docs.rs entry"),
            "read https://docs.rs entry"
        );
    }

    #[test]
    fn normalize_title_rewrites_mapping_breaking_colons() {
        assert_eq!(
            normalize_title("finish refactor: promote sync-git seam"),
            "finish refactor; promote sync-git seam"
        );
        assert_eq!(normalize_title("fix parser:"), "fix parser");
        assert_eq!(normalize_title("a :: b"), "a ; b");
        assert_eq!(normalize_title("fix parser :"), "fix parser");
    }

    #[test]
    fn normalize_title_drops_comment_starting_hashes() {
        assert_eq!(normalize_title("fix #123 now"), "fix 123 now");
        assert_eq!(normalize_title("# lead hash"), "lead hash");
        assert_eq!(normalize_title("close c# ticket"), "close c# ticket");
    }

    #[test]
    fn normalize_title_strips_unsafe_leading_indicator_chars() {
        assert_eq!(normalize_title("- do it"), "do it");
        assert_eq!(normalize_title("[wip] fix"), "wip] fix");
        assert_eq!(normalize_title("\"quoted start"), "quoted start");
        assert_eq!(normalize_title("? open question"), "open question");
        assert_eq!(normalize_title("-x marks the spot"), "-x marks the spot");
    }

    #[test]
    fn normalize_title_collapses_whitespace_onto_one_line() {
        assert_eq!(normalize_title("a\nb: c"), "a b; c");
        assert_eq!(normalize_title("tab\there"), "tab here");
    }

    #[test]
    fn normalize_title_falls_back_to_default_when_nothing_survives() {
        for raw in [":", ":::", "#", " : ", "- ", ""] {
            assert_eq!(normalize_title(raw), "n/a", "input: {raw:?}");
        }
    }

    #[test]
    fn title_was_normalized_ignores_case_and_outer_whitespace_changes() {
        assert!(!title_was_normalized("  Fix The THING  "));
        assert!(!title_was_normalized("time is 3:30pm"));
        assert!(title_was_normalized("finish refactor: promote seam"));
        assert!(title_was_normalized("fix #123 now"));
        assert!(title_was_normalized(""));
    }
}
