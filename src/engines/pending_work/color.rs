//! Terminal color policy + a shared ANSI paint helper, used by anything that
//! renders human-facing confirmation/outcome text (`session` dispatch
//! outcomes, `add` confirmations). ANSI is gated by an explicit `on` bool
//! resolved once at the edge (`use_color`) so callers stay unit-testable.

use anstyle::Style;

use crate::cli::ColorChoice;

/// Resolve the effective color setting once, at the edge. `NO_COLOR` wins;
/// `CLICOLOR_FORCE` forces on; `Auto` falls back to stdout TTY detection.
pub(in crate::engines::pending_work) fn use_color(choice: ColorChoice) -> bool {
    use std::io::IsTerminal;
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    if std::env::var_os("CLICOLOR_FORCE").is_some() {
        return true;
    }
    match choice {
        ColorChoice::Always => true,
        ColorChoice::Never => false,
        ColorChoice::Auto => std::io::stdout().is_terminal(),
    }
}

/// Bold + `color`; degrades to Markdown `**bold**` when `on` is false.
pub(in crate::engines::pending_work) fn paint(
    text: &str,
    color: impl Into<anstyle::Color>,
    on: bool,
) -> String {
    if !on {
        return format!("**{text}**");
    }
    let style = Style::new().bold().fg_color(Some(color.into()));
    format!("{}{}{}", style.render(), text, style.render_reset())
}

#[cfg(test)]
mod tests {
    use anstyle::AnsiColor;

    use super::*;

    #[test]
    fn use_color_never_returns_false() {
        assert!(!use_color(ColorChoice::Never));
        // Auto/Always depend on env/TTY and are not asserted here.
    }

    #[test]
    fn paint_off_degrades_to_markdown_bold() {
        assert_eq!(paint("x", AnsiColor::Green, false), "**x**");
    }

    #[test]
    fn paint_on_emits_ansi() {
        assert!(paint("x", AnsiColor::Green, true).contains('\u{1b}'));
    }
}
