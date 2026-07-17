use anstyle::{Ansi256Color, Style};

use crate::cli::ColorChoice;

/// Provides the orange palette entry used for list item identifiers.
pub(in crate::engines::pending_work) const ID_ORANGE: Ansi256Color = Ansi256Color(208);

/// Resolves color at the CLI edge. `NO_COLOR` takes precedence over `CLICOLOR_FORCE`; `Auto` checks
/// stdout.
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

/// Applies bold ANSI color, or Markdown bold when color is disabled.
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
