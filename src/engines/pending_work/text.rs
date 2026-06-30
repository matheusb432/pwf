// Pure string helpers: title inference, placeholder detection, section keywords,
// line-number math. No I/O.

use std::{path::Path, sync::LazyLock};

use regex::Regex;

static PLACEHOLDER_PROMPT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(^\s*\[!\]\s*TODO\b|^\s*TODO\b|definir prompt|define prompt|tbd)").unwrap()
});
static DASH_UNDERSCORE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[-_]+").unwrap());
static DATE_SLUG_PREFIX_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\d{4}-\d{2}-\d{2}-").unwrap());

/// True for an empty prompt or a recognized placeholder (TODO, tbd, "define prompt").
pub fn is_placeholder_prompt(prompt: &str) -> bool {
    if prompt.trim().is_empty() {
        return true;
    }
    PLACEHOLDER_PROMPT_RE.is_match(prompt)
}

/// Casefolds a title to its canonical lowercase form. Single source of truth so
/// inferred and explicit titles store identically — every title write-point routes
/// through here, and existing non-lowercase titles were folded to match.
pub fn normalize_title(title: &str) -> String {
    title.to_lowercase()
}

/// Returns the inferred title for a pending-work prompt.
pub fn inferred_title(text: &str) -> String {
    super::prompt_format::inferred_title(text)
}

/// Renders the standard pending-work note body from a prompt.
pub fn goals_body(prompt: &str) -> String {
    super::prompt_format::goals_body(prompt)
}

/// The note body stored for a new item: the `## Goals` template, unless the prompt
/// is a placeholder — those are stored verbatim so `is_placeholder_prompt` still
/// flags the item as needing a real prompt (the Goals wrapper would hide it).
pub fn note_body(prompt: &str) -> String {
    super::prompt_format::note_body(prompt)
}

/// "glep-shimeji" -> "glep shimeji"
pub fn project_title_prefix(name: &str) -> String {
    DASH_UNDERSCORE_RE
        .replace_all(name, " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Stem minus YYYY-MM-DD- prefix, words joined, prefixed "continue ".
pub fn handoff_title_from_path(path: &str) -> String {
    let stem = Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let slug = DATE_SLUG_PREFIX_RE.replace(stem, "");
    let words: Vec<&str> = DASH_UNDERSCORE_RE
        .split(&slug)
        .filter(|w| !w.is_empty())
        .collect();
    if words.is_empty() {
        "continue handoff".to_string()
    } else {
        format!("continue {}", words.join(" "))
    }
}

pub fn get_title_from_continue_path(project_name: &str, path: &str) -> String {
    let stem = Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let slug = DATE_SLUG_PREFIX_RE.replace(stem, "");
    let excluded = ["kickoff", "handoff", "plan"];
    let words: Vec<String> = DASH_UNDERSCORE_RE
        .split(&slug)
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase())
        .filter(|w| !excluded.contains(&w.as_str()))
        .collect();
    if words.is_empty() {
        format!("{} plan", project_title_prefix(project_name))
    } else {
        format!("{} {}", project_title_prefix(project_name), words.join(" "))
    }
}

/// 1-based line number at byte offset `index` in `text`.
pub fn line_number(text: &str, index: usize) -> usize {
    if index == 0 {
        return 1;
    }
    1 + text[..index].matches('\n').count() // inputs are normalized to LF
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAX_RENDERED_TITLE_CHARS: usize = 81;

    #[test]
    fn inferred_title_keeps_full_prompt_without_ampersand() {
        // No '&' → the whole prompt is the title: no word cut, lowercased.
        assert_eq!(
            inferred_title("Build the data filter to let users sort"),
            "build the data filter to let users sort"
        );
        assert_eq!(inferred_title("add startup toggle"), "add startup toggle");
    }

    #[test]
    fn inferred_title_cuts_at_first_lane_marker_only() {
        assert_eq!(
            inferred_title("create engine feature to add update task / make it idempotent"),
            "create engine feature to add update task"
        );
        assert_eq!(inferred_title("a / b / c"), "a");
        // Comma / colon / semicolon are NOT cut markers anymore.
        assert_eq!(
            inferred_title("fix bug: empty prompt"),
            "fix bug: empty prompt"
        );
    }

    #[test]
    fn inferred_title_collapses_whitespace_and_lowercases() {
        assert_eq!(
            inferred_title("  Refactor   Help  Command  "),
            "refactor help command"
        );
    }

    #[test]
    fn inferred_title_empty_lead_uses_pending_work_fallback() {
        assert_eq!(inferred_title("/ only second"), "pending work");
    }

    #[test]
    fn inferred_title_caps_long_prompt_without_marker_at_word_boundary() {
        // CFG-0075 regression: a long prompt with no marker must NOT become a giant
        // title. Cap on a word boundary, append an ellipsis, lowercase.
        let prompt = "Continue the PowerShell to Rust port into the cfgtool CLI (scripts/cfgtool), using the shipped gaming domain as the template, porting domain-by-domain smallest first";
        let title = inferred_title(prompt);
        assert_eq!(
            title,
            "continue the powershell to rust port into the cfgtool cli (scripts/cfgtool),…"
        );
        // Whole title (incl. ellipsis) stays bounded for any input.
        assert!(title.chars().count() <= MAX_RENDERED_TITLE_CHARS);
        // The kept text is a genuine prefix of the source (no mid-word cut).
        assert!(
            prompt
                .to_lowercase()
                .starts_with(title.trim_end_matches('…'))
        );
    }

    #[test]
    fn inferred_title_caps_long_lead_clause_before_marker() {
        let prompt = "HUMAN: Start studying AZ-104 Section 02 - Storage. Begin with the Storage MOC, then cover Storage Accounts / Redundancy / Security";
        let title = inferred_title(prompt);
        assert_eq!(
            title,
            "human: start studying az-104 section 02 - storage. begin with the storage moc,…"
        );
        assert!(title.chars().count() <= MAX_RENDERED_TITLE_CHARS);
    }

    #[test]
    fn inferred_title_short_prompt_passes_through_uncapped() {
        // At/under the cap → verbatim, no ellipsis.
        assert_eq!(
            inferred_title("add a startup toggle to the settings page"),
            "add a startup toggle to the settings page"
        );
    }

    #[test]
    fn inferred_title_caps_single_overlong_word_on_char_boundary() {
        // A first "word" longer than the cap (e.g. a URL) has no space to break on;
        // fall back to a char-boundary cut so the title is still bounded and valid.
        let word = "x".repeat(200);
        let title = inferred_title(&word);
        assert!(title.ends_with('…'));
        assert_eq!(title.chars().count(), MAX_RENDERED_TITLE_CHARS);
    }

    #[test]
    fn inferred_title_is_always_bounded() {
        // PWF-0031 invariant: for any input, the inferred title's char count never
        // exceeds the cap plus the appended ellipsis. A
        // deterministic adversarial table (no proptest dep): long no-boundary, long
        // lead before marker, all separators, multi-byte emoji, many spaces, empty.
        let cases = [
            "x".repeat(500),                           // long, no boundary
            format!("{} / tail", "word ".repeat(200)), // long lead before marker
            "/ ".repeat(300),                          // all separators
            "💥".repeat(300),                          // multi-byte, no spaces
            "a ".repeat(300),                          // many word boundaries
            String::new(),                             // empty
        ];
        for input in cases {
            let t = inferred_title(&input);
            assert!(
                t.chars().count() <= MAX_RENDERED_TITLE_CHARS,
                "input bound violated ({} chars): {t}",
                t.chars().count()
            );
        }
    }

    #[test]
    fn normalize_title_lowercases_and_leaves_lowercase_untouched() {
        assert_eq!(normalize_title("HUMAN: Do AZ-104"), "human: do az-104");
        assert_eq!(normalize_title("already lower"), "already lower");
    }

    #[test]
    fn placeholder_prompt_detection() {
        assert!(is_placeholder_prompt(""));
        assert!(is_placeholder_prompt("TODO define this"));
        assert!(is_placeholder_prompt("definir prompt"));
        assert!(!is_placeholder_prompt("add startup toggle"));
    }

    #[test]
    fn handoff_title_from_path_strips_date_prefix() {
        assert_eq!(
            handoff_title_from_path("docs/handoffs/2026-01-01-api-cleanup.md"),
            "continue api cleanup"
        );
    }

    #[test]
    fn goals_body_single_segment_without_ampersand() {
        assert_eq!(
            goals_body("add startup toggle"),
            "## Goals\n- add startup toggle"
        );
    }

    #[test]
    fn goals_body_one_bullet_per_slash_lane() {
        assert_eq!(
            goals_body("create engine feature to add update task / make it idempotent"),
            "## Goals\n- create engine feature to add update task\n- make it idempotent"
        );
    }

    #[test]
    fn goals_body_preserves_ampersands_as_text() {
        assert_eq!(goals_body("a & b"), "## Goals\n- a & b");
    }

    #[test]
    fn note_body_wraps_a_normal_prompt() {
        assert_eq!(
            note_body("add startup toggle"),
            "## Goals\n- add startup toggle"
        );
        assert_eq!(note_body("a / b"), "## Goals\n- a\n- b");
    }

    #[test]
    fn note_body_keeps_placeholder_raw_so_it_stays_detectable() {
        // Wrapping a placeholder would hide it from is_placeholder_prompt at
        // read time, silently making an un-written item launchable.
        assert_eq!(note_body("TODO"), "TODO");
        assert!(is_placeholder_prompt(&note_body("TODO")));
        assert!(is_placeholder_prompt(&note_body("tbd")));
        assert!(is_placeholder_prompt(&note_body("define prompt")));
    }
}
