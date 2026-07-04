//! Presentation-only reformat of `add`/`remove`/`update`'s confirmation text,
//! applied at the single call site that reaches a human terminal directly
//! (`run.rs`'s outer `run()`). Never applied to those actions' own plain-text
//! return values — `add`'s is also the data contract the handoff/done
//! cross-engine seams parse id-first-bracket-on-line-one out of
//! (`parse_added_id` in `engines/handoff/pw_bridge.rs`); wrapping it in ANSI
//! there would plant a stray `[` before the id's own bracket and break that
//! parse. See AGENTS.md's cross-engine-seams note (PWF-0087).

use anstyle::Color;

use super::super::color::paint;

/// Reformat `"<VERB> PWF TASK [<id>] <rest>\n<remaining lines>"` into
/// `"<label>: <id> <rest>\n<remaining lines>"`, with the id and rest painted
/// bold + `color` when `on` — the id leads the highlighted span so it pops
/// out among long prompts.
pub(in crate::engines::pending_work) fn render_confirmation(
    text: &str,
    label: &str,
    color: impl Into<Color>,
    on: bool,
) -> String {
    let mut lines = text.lines();
    let Some(first) = lines.next() else {
        return text.to_string();
    };
    let Some((id, rest)) = split_id(first) else {
        return text.to_string();
    };
    let mut out = format!("{label}: {}\n", paint(&format!("{id} {rest}"), color, on));
    for line in lines {
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Split `"<VERB> PWF TASK [<id>] <rest>"` into `(<id>, <rest>)`.
fn split_id(first_line: &str) -> Option<(&str, &str)> {
    let start = first_line.find('[')?;
    let end = start + 1 + first_line[start + 1..].find(']')?;
    let id = &first_line[start + 1..end];
    let rest = first_line[end + 1..].trim_start();
    Some((id, rest))
}

#[cfg(test)]
mod tests {
    use anstyle::AnsiColor;

    use super::*;

    const RAW: &str = "ADDED PWF TASK [PWF-0087] pwf :: color tui output when adding pwf task\n  file: /x/PWF-0087.md\n";

    #[test]
    fn plain_prefixes_label_with_no_leading_blank_line() {
        let out = render_confirmation(RAW, "Added pwf task", AnsiColor::Green, false);
        assert_eq!(
            out,
            "Added pwf task: **PWF-0087 pwf :: color tui output when adding pwf task**\n  file: /x/PWF-0087.md\n"
        );
    }

    #[test]
    fn colored_has_no_leading_blank_line_and_keeps_id_and_rest_intact() {
        let out = render_confirmation(RAW, "Added pwf task", AnsiColor::Green, true);
        assert!(out.starts_with("Added pwf task: "), "got: {out}");
        assert!(!out.starts_with('\n'), "got: {out}");
        assert!(out.contains('\u{1b}'), "got: {out}");
        assert!(out.contains("PWF-0087"), "got: {out}");
        assert!(
            out.contains("pwf :: color tui output when adding pwf task"),
            "got: {out}"
        );
        assert!(out.contains("  file: /x/PWF-0087.md\n"), "got: {out}");
    }

    #[test]
    fn different_labels_and_colors_apply_independently() {
        let removed = "REMOVED PWF TASK [PWF-0002] pwf :: stale task\n  deleted: /x.md\n";
        let out = render_confirmation(removed, "Removed pwf task", AnsiColor::Red, false);
        assert_eq!(
            out,
            "Removed pwf task: **PWF-0002 pwf :: stale task**\n  deleted: /x.md\n"
        );

        let updated = "UPDATED PWF TASK [PWF-0003] pwf :: renamed\n";
        let out = render_confirmation(updated, "Updated pwf task", AnsiColor::Blue, false);
        assert_eq!(out, "Updated pwf task: **PWF-0003 pwf :: renamed**\n");
    }

    #[test]
    fn malformed_first_line_returns_input_unchanged() {
        let text = "oops no brackets here\n";
        assert_eq!(
            render_confirmation(text, "Added pwf task", AnsiColor::Green, false),
            text
        );
    }

    #[test]
    fn empty_input_returns_input_unchanged() {
        assert_eq!(
            render_confirmation("", "Added pwf task", AnsiColor::Green, false),
            ""
        );
    }
}
