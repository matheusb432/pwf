//! Collects confirmation answers through Dialoguer.

use dialoguer::{Confirm, console::Term};
use pwf_client::confirmation::{Confirmation, ConfirmationPrompt};

use crate::{
    console::Console,
    render::{self, ConfirmationDialog, ConfirmationTheme},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ConfirmationAnswer {
    Accepted,
    Declined,
}

impl From<Option<bool>> for ConfirmationAnswer {
    fn from(answer: Option<bool>) -> Self {
        match answer {
            Some(true) => Self::Accepted,
            Some(false) | None => Self::Declined,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ConfirmationMode {
    AssumeYes,
    Prompt,
}

pub(crate) struct CliConfirmationClient {
    console: Console,
    mode: ConfirmationMode,
}

impl CliConfirmationClient {
    pub(crate) fn new(console: Console, mode: ConfirmationMode) -> Self {
        Self { console, mode }
    }
}

impl ConfirmationPrompt for CliConfirmationClient {
    type Error = dialoguer::Error;

    fn confirm(&self, confirmation: &Confirmation) -> Result<bool, Self::Error> {
        match self.mode {
            ConfirmationMode::AssumeYes => Ok(true),
            ConfirmationMode::Prompt => self
                .console
                .confirm(&render::confirmation_dialog(confirmation))
                .map(|answer| answer == ConfirmationAnswer::Accepted),
        }
    }
}

pub(crate) fn interact(
    dialog: &ConfirmationDialog,
    color: bool,
) -> dialoguer::Result<ConfirmationAnswer> {
    let terminal = Term::stderr();
    terminal.write_line(&dialog.render(color, Some(usize::from(terminal.size().1))))?;
    terminal.write_line("")?;
    let theme = ConfirmationTheme::default();
    Confirm::with_theme(&theme)
        .with_prompt(dialog.question)
        .default(dialog.default.accepted())
        .interact_on_opt(&terminal)
        .map(ConfirmationAnswer::from)
}

pub(crate) fn prompt_error(context: &str, source: dialoguer::Error) -> anyhow::Error {
    let detail = source.to_string();
    anyhow::Error::new(source).context(format!("{context} confirmation failed: {detail}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_is_a_declined_answer() {
        assert_eq!(ConfirmationAnswer::from(None), ConfirmationAnswer::Declined);
    }
}
