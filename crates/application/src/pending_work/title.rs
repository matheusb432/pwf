use prompt_lanes::parse;
use pwf_models::pending_work::TaskTitle;

const MAX_TITLE_CHARS: usize = 80;
const YAML_UNSAFE_LEADING_CHARS: [char; 16] = [
    ',', '[', ']', '{', '}', '#', '&', '*', '!', '|', '>', '\'', '"', '%', '@', '`',
];

#[must_use]
pub(super) fn normalize(title: &str) -> String {
    let safe = yaml_plain_scalar(&title.to_lowercase());
    if safe.is_empty() {
        TaskTitle::default().to_string()
    } else {
        TaskTitle::try_new(safe)
            .expect("YAML-safe normalization returns a non-empty canonical title")
            .to_string()
    }
}

#[must_use]
pub(super) fn was_normalized(raw: &str) -> bool {
    normalize(raw) != raw.trim().to_lowercase()
}

#[must_use]
pub(super) fn inferred(prompt: &str) -> String {
    normalize(&parse(prompt).capped_title(MAX_TITLE_CHARS))
}

fn yaml_plain_scalar(title: &str) -> String {
    let collapsed = title.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out = String::with_capacity(collapsed.len());
    let mut characters = collapsed.chars().peekable();
    let mut opens_comment = true;
    while let Some(character) = characters.next() {
        match character {
            ':' => {
                let mut run_length = 1;
                while characters.next_if_eq(&':').is_some() {
                    run_length += 1;
                }
                match characters.peek() {
                    None => {}
                    Some(' ') => out.push(';'),
                    Some(_) => out.extend(std::iter::repeat_n(':', run_length)),
                }
                opens_comment = false;
            }
            '#' if opens_comment => {}
            _ => {
                out.push(character);
                opens_comment = character == ' ';
            }
        }
    }
    strip_unsafe_leading_chars(out.trim_end()).to_string()
}

fn strip_unsafe_leading_chars(mut value: &str) -> &str {
    loop {
        value = value.trim_start_matches(' ');
        let mut characters = value.chars();
        let Some(first) = characters.next() else {
            return value;
        };
        let unsafe_lead = YAML_UNSAFE_LEADING_CHARS.contains(&first)
            || (matches!(first, '-' | '?' | ':')
                && characters.next().is_none_or(|second| second == ' '));
        if !unsafe_lead {
            return value;
        }
        value = &value[first.len_utf8()..];
    }
}

#[cfg(test)]
mod tests {
    use super::{inferred, normalize, was_normalized};

    const MAX_RENDERED_TITLE_CHARS: usize = 81;

    #[test]
    fn inference_keeps_full_prompt_without_a_lane_marker() {
        assert_eq!(
            inferred("Build the data filter to let users sort"),
            "build the data filter to let users sort"
        );
        assert_eq!(inferred("add startup toggle"), "add startup toggle");
    }

    #[test]
    fn inference_cuts_at_the_first_lane_marker() {
        assert_eq!(
            inferred("create engine feature to add update task / make it idempotent"),
            "create engine feature to add update task"
        );
        assert_eq!(inferred("a / b / c"), "a");
        assert_eq!(inferred("fix bug: empty prompt"), "fix bug; empty prompt");
    }

    #[test]
    fn inference_collapses_whitespace_and_lowercases() {
        assert_eq!(
            inferred("  Refactor   Help  Command  "),
            "refactor help command"
        );
    }

    #[test]
    fn inference_empty_lead_uses_the_canonical_fallback() {
        assert_eq!(inferred("/ only second"), "n/a");
        assert_eq!(inferred("/c context"), "n/a");
    }

    #[test]
    fn inference_caps_long_prompt_without_marker_at_word_boundary() {
        let prompt = "Continue the PowerShell to Rust port into the cfgtool CLI (scripts/cfgtool), using the shipped gaming domain as the template, porting domain-by-domain smallest first";
        let title = inferred(prompt);
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
    fn inference_caps_long_lead_clause_before_marker() {
        let prompt = "HUMAN: Start studying AZ-104 Section 02 - Storage. Begin with the Storage MOC, then cover Storage Accounts / Redundancy / Security";
        let title = inferred(prompt);
        assert_eq!(
            title,
            "human; start studying az-104 section 02 - storage. begin with the storage moc,…"
        );
        assert!(title.chars().count() <= MAX_RENDERED_TITLE_CHARS);
    }

    #[test]
    fn inference_short_prompt_passes_through_uncapped() {
        assert_eq!(
            inferred("add a startup toggle to the settings page"),
            "add a startup toggle to the settings page"
        );
    }

    #[test]
    fn inference_caps_single_overlong_word_on_char_boundary() {
        let word = "x".repeat(200);
        let title = inferred(&word);
        assert!(title.ends_with('…'));
        assert_eq!(title.chars().count(), MAX_RENDERED_TITLE_CHARS);
    }

    #[test]
    fn inference_is_always_bounded() {
        let cases = [
            "x".repeat(500),
            format!("{} / tail", "word ".repeat(200)),
            "/ ".repeat(300),
            "💥".repeat(300),
            "a ".repeat(300),
            String::new(),
        ];
        for input in cases {
            let title = inferred(&input);
            assert!(
                title.chars().count() <= MAX_RENDERED_TITLE_CHARS,
                "input bound violated ({} chars): {title}",
                title.chars().count()
            );
        }
    }

    #[test]
    fn normalization_lowercases_and_leaves_safe_titles_untouched() {
        assert_eq!(normalize("HUMAN: Do AZ-104"), "human; do az-104");
        assert_eq!(normalize("already lower"), "already lower");
        assert_eq!(normalize("time is 3:30pm"), "time is 3:30pm");
        assert_eq!(normalize("fix foo::bar panic"), "fix foo::bar panic");
        assert_eq!(
            normalize("read https://docs.rs entry"),
            "read https://docs.rs entry"
        );
    }

    #[test]
    fn normalization_rewrites_mapping_breaking_colons() {
        assert_eq!(
            normalize("finish refactor: promote sync-git seam"),
            "finish refactor; promote sync-git seam"
        );
        assert_eq!(normalize("fix parser:"), "fix parser");
        assert_eq!(normalize("a :: b"), "a ; b");
        assert_eq!(normalize("fix parser :"), "fix parser");
    }

    #[test]
    fn normalization_drops_comment_starting_hashes() {
        assert_eq!(normalize("fix #123 now"), "fix 123 now");
        assert_eq!(normalize("# lead hash"), "lead hash");
        assert_eq!(normalize("close c# ticket"), "close c# ticket");
    }

    #[test]
    fn normalization_strips_unsafe_leading_indicator_chars() {
        assert_eq!(normalize("- do it"), "do it");
        assert_eq!(normalize("[wip] fix"), "wip] fix");
        assert_eq!(normalize("\"quoted start"), "quoted start");
        assert_eq!(normalize("? open question"), "open question");
        assert_eq!(normalize("-x marks the spot"), "-x marks the spot");
    }

    #[test]
    fn normalization_collapses_whitespace_onto_one_line() {
        assert_eq!(normalize("a\nb: c"), "a b; c");
        assert_eq!(normalize("tab\there"), "tab here");
    }

    #[test]
    fn normalization_falls_back_when_nothing_survives() {
        for raw in [":", ":::", "#", " : ", "- ", ""] {
            assert_eq!(normalize(raw), "n/a", "input: {raw:?}");
        }
    }

    #[test]
    fn normalization_notice_ignores_case_and_outer_whitespace_changes() {
        assert!(!was_normalized("  Fix The THING  "));
        assert!(!was_normalized("time is 3:30pm"));
        assert!(was_normalized("finish refactor: promote seam"));
        assert!(was_normalized("fix #123 now"));
        assert!(was_normalized(""));
    }
}
