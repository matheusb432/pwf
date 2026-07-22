//! Resolves terminal capabilities once at the process edge.

use crate::confirm::{self, Confirmation, DefaultAnswer};

/// Terminal capabilities resolved at the binary edge.
///
/// Engine code receives this value instead of sniffing process TTY or color
/// environment state, so in-process callers stay deterministic.
#[derive(Clone, Copy, Debug)]
pub struct Console {
    interactive: bool,
    color_forced: Option<bool>,
    stdout_terminal: bool,
}

impl Console {
    /// Reads process TTY and color environment state; call only from the binary edge.
    pub fn from_terminal() -> Self {
        use std::io::IsTerminal;

        let color_forced = if std::env::var_os("NO_COLOR").is_some() {
            Some(false)
        } else if std::env::var_os("CLICOLOR_FORCE").is_some() {
            Some(true)
        } else {
            None
        };
        Self {
            interactive: std::io::stdin().is_terminal(),
            color_forced,
            stdout_terminal: std::io::stdout().is_terminal(),
        }
    }

    /// A non-interactive console that renders plain markdown.
    pub fn plain() -> Self {
        Self {
            interactive: false,
            color_forced: None,
            stdout_terminal: false,
        }
    }

    /// Asks on the terminal when interactive; otherwise reports `NonInteractive`.
    pub fn confirm(self, question: &str, default: DefaultAnswer) -> Confirmation {
        if !self.interactive {
            return Confirmation::NonInteractive;
        }
        confirm::prompt(question, default)
    }

    /// Whether auto-detected stdout styling is enabled.
    pub fn color(self) -> bool {
        self.color_forced.unwrap_or(self.stdout_terminal)
    }

    /// Styling with an explicit request; forcing environment variables still win.
    pub fn color_with(self, requested: Option<bool>) -> bool {
        self.color_forced
            .unwrap_or_else(|| requested.unwrap_or(self.stdout_terminal))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_console_never_prompts_and_never_styles() {
        let console = Console::plain();

        assert_eq!(
            console.confirm("proceed?", DefaultAnswer::Yes),
            Confirmation::NonInteractive
        );
        assert!(!console.color());
        assert!(!console.color_with(None));
    }

    #[test]
    fn color_with_honors_explicit_request_without_forcing_env() {
        let console = Console::plain();

        assert!(console.color_with(Some(true)));
        assert!(!console.color_with(Some(false)));
    }
}
