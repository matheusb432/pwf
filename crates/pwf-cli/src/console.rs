//! Resolves terminal capabilities once at the process edge.

use crate::confirmation::{ConfirmationAnswer, ConfirmationDialog, ConfirmationMode};

/// Terminal capabilities resolved at the binary edge.
///
/// Engine code receives this value instead of sniffing process TTY or color
/// environment state, so in-process callers stay deterministic.
#[derive(Clone, Copy, Debug)]
pub struct Console {
    interactive: bool,
    color_forced: Option<bool>,
    stderr_terminal: bool,
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
        let stderr_terminal = std::io::stderr().is_terminal();
        Self {
            interactive: std::io::stdin().is_terminal() && stderr_terminal,
            color_forced,
            stderr_terminal,
            stdout_terminal: std::io::stdout().is_terminal(),
        }
    }

    /// A non-interactive console that renders plain markdown.
    #[cfg(test)]
    fn plain() -> Self {
        Self {
            interactive: false,
            color_forced: None,
            stderr_terminal: false,
            stdout_terminal: false,
        }
    }

    pub(crate) fn confirmation_mode(self, assume_yes: bool) -> anyhow::Result<ConfirmationMode> {
        if assume_yes {
            return Ok(ConfirmationMode::AssumeYes);
        }
        if self.interactive {
            return Ok(ConfirmationMode::Prompt);
        }
        anyhow::bail!("interactive confirmation requires a terminal; rerun with --yes")
    }

    pub(crate) fn confirm(
        self,
        dialog: &ConfirmationDialog,
    ) -> dialoguer::Result<ConfirmationAnswer> {
        dialog.interact(self.color_forced.unwrap_or(self.stderr_terminal))
    }

    /// Whether auto-detected stdout styling is enabled.
    pub(crate) fn color(self) -> bool {
        self.color_forced.unwrap_or(self.stdout_terminal)
    }

    /// Styling with an explicit request; forcing environment variables still win.
    pub(crate) fn color_with(self, requested: Option<bool>) -> bool {
        self.color_forced
            .unwrap_or_else(|| requested.unwrap_or(self.stdout_terminal))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_console_requires_an_explicit_confirmation_bypass() {
        let console = Console::plain();

        assert_eq!(
            console.confirmation_mode(true).unwrap(),
            ConfirmationMode::AssumeYes
        );
        assert!(
            console
                .confirmation_mode(false)
                .unwrap_err()
                .to_string()
                .contains("--yes")
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
