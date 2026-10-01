//! Resolves terminal capabilities once at the process edge.

use crate::{
    confirmation::{self, ConfirmationAnswer, ConfirmationMode},
    render::ConfirmationDialog,
};

#[derive(Clone, Copy, Debug)]
pub struct Console {
    color_forced: Option<bool>,
    stderr_terminal: bool,
    stdin_terminal: bool,
    stdout_terminal: bool,
    stdout_columns: Option<usize>,
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
        let stdin_terminal = std::io::stdin().is_terminal();
        let stdout_terminal = std::io::stdout().is_terminal();
        Self {
            color_forced,
            stderr_terminal,
            stdin_terminal,
            stdout_terminal,
            stdout_columns: stdout_terminal
                .then(|| usize::from(dialoguer::console::Term::stdout().size().1)),
        }
    }

    /// A non-interactive console that renders plain markdown.
    #[cfg(test)]
    fn plain() -> Self {
        Self {
            color_forced: None,
            stderr_terminal: false,
            stdin_terminal: false,
            stdout_terminal: false,
            stdout_columns: None,
        }
    }

    pub(crate) fn confirmation_mode(self, assume_yes: bool) -> anyhow::Result<ConfirmationMode> {
        if assume_yes {
            return Ok(ConfirmationMode::AssumeYes);
        }
        if self.confirmation_terminal() {
            return Ok(ConfirmationMode::Prompt);
        }
        anyhow::bail!("interactive confirmation requires a terminal; rerun with --yes")
    }

    pub(crate) fn confirm(
        self,
        dialog: &ConfirmationDialog,
    ) -> dialoguer::Result<ConfirmationAnswer> {
        if !self.confirmation_terminal() {
            return Err(std::io::Error::other(
                "interactive confirmation requires a terminal; rerun with --yes",
            )
            .into());
        }
        confirmation::interact(dialog, self.color_forced.unwrap_or(self.stderr_terminal))
    }

    pub(crate) fn error_color(self) -> bool {
        self.color_forced.unwrap_or(self.stderr_terminal)
    }

    pub(crate) const fn stdout_terminal(self) -> bool {
        self.stdout_terminal
    }

    pub(crate) const fn fullscreen_terminal(self) -> bool {
        self.stdin_terminal && self.stdout_terminal
    }

    const fn confirmation_terminal(self) -> bool {
        self.stdin_terminal && self.stderr_terminal
    }

    pub(crate) const fn stdout_columns(self) -> Option<usize> {
        self.stdout_columns
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
