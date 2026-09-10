//! Resolves terminal capabilities once at the process edge.

use crate::confirmation::{ConfirmationAnswer, ConfirmationDialog, ConfirmationMode};

#[derive(Clone, Copy, Debug)]
pub struct Console {
    interactive: bool,
    color_forced: Option<bool>,
    stderr_terminal: bool,
    stdout_terminal: bool,
}

impl Console {
    /// Reads process TTY and color environment state; call only from the binary edge.
    #[must_use]
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

    pub(crate) fn error_color(self) -> bool {
        self.color_forced.unwrap_or(self.stderr_terminal)
    }

    /// Whether auto-detected stdout styling is enabled.
    pub(crate) fn color(self) -> bool {
        self.color_forced.unwrap_or(self.stdout_terminal)
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
    }
}
