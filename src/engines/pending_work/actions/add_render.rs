//! Presentation-only reformat of `add`'s confirmation text, applied at the
//! single call site that reaches a human terminal directly (`run.rs`'s
//! `Action::Add` arm). Never applied to `add_pending_work_item`'s plain-text
//! return value itself — that string is also the data contract the
//! handoff/check cross-engine seams parse id-first-bracket-on-line-one out of
//! (`parse_added_id` in `engines/handoff/pw_bridge.rs`); wrapping it in ANSI
//! there would plant a stray `[` before the id's own bracket and break that
//! parse. See AGENTS.md's cross-engine-seams note (PWF-0087).

use anstyle::Ansi256Color;

use super::super::color::paint;

/// A common "orange" in the 256-color palette (no orange in the basic 16).
const ID_ORANGE: Ansi256Color = Ansi256Color(208);

/// Reformat `"ADDED PWF TASK [<id>] <rest>\n<remaining lines>"` into
/// `"\n<id> <rest>\n<remaining lines>"`, with the id painted bold + orange
/// when `on` — the id leads so it pops out among long prompts, and the
/// leading blank line sets it apart from whatever printed before it.
pub(in crate::engines::pending_work) fn render_add_confirmation(text: &str, on: bool) -> String {
    let mut lines = text.lines();
    let Some(first) = lines.next() else {
        return text.to_string();
    };
    let Some((id, rest)) = split_id(first) else {
        return text.to_string();
    };
    let mut out = format!("\n{} {rest}\n", paint(id, ID_ORANGE, on));
    for line in lines {
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Split `"ADDED PWF TASK [<id>] <rest>"` into `(<id>, <rest>)`.
fn split_id(first_line: &str) -> Option<(&str, &str)> {
    let start = first_line.find('[')?;
    let end = start + 1 + first_line[start + 1..].find(']')?;
    let id = &first_line[start + 1..end];
    let rest = first_line[end + 1..].trim_start();
    Some((id, rest))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RAW: &str = "ADDED PWF TASK [PWF-0087] pwf :: color tui output when adding pwf task\n  file: /x/PWF-0087.md\n";

    #[test]
    fn plain_moves_id_to_front_with_leading_blank_line() {
        let out = render_add_confirmation(RAW, false);
        assert_eq!(
            out,
            "\n**PWF-0087** pwf :: color tui output when adding pwf task\n  file: /x/PWF-0087.md\n"
        );
    }

    #[test]
    fn colored_leads_with_blank_line_and_keeps_id_and_rest_intact() {
        let out = render_add_confirmation(RAW, true);
        assert!(out.starts_with('\n'), "got: {out}");
        assert!(out.contains('\u{1b}'), "got: {out}");
        assert!(out.contains("PWF-0087"), "got: {out}");
        assert!(
            out.contains("pwf :: color tui output when adding pwf task"),
            "got: {out}"
        );
        assert!(out.contains("  file: /x/PWF-0087.md\n"), "got: {out}");
    }

    #[test]
    fn malformed_first_line_returns_input_unchanged() {
        let text = "oops no brackets here\n";
        assert_eq!(render_add_confirmation(text, false), text);
    }

    #[test]
    fn empty_input_returns_input_unchanged() {
        assert_eq!(render_add_confirmation("", false), "");
    }
}
