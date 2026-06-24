use super::super::model::Action;
use crate::{cli::Args, engines::pending_work::errors::PendingWorkError};

#[derive(Debug, Clone)]
pub struct PendingWorkCommand {
    action: Action,
    args: Args,
}

impl PendingWorkCommand {
    pub(crate) fn new(action: Action, args: Args) -> Self {
        Self { action, args }
    }

    pub(in crate::engines::pending_work) fn from_args_typed(
        args: &Args,
    ) -> Result<Self, PendingWorkError> {
        let action_raw = args
            .action
            .as_deref()
            .ok_or(PendingWorkError::MissingSubcommand)?
            .to_ascii_lowercase();
        let action: Action = action_raw
            .parse()
            .map_err(|_| PendingWorkError::UnknownAction {
                action: action_raw.clone(),
            })?;
        Ok(Self::new(action, args.clone()))
    }

    pub(crate) fn action(&self) -> &Action {
        &self.action
    }

    pub(crate) fn args(&self) -> &Args {
        &self.args
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::pending_work::errors::PendingWorkError;

    #[test]
    fn missing_action_returns_typed_error_with_legacy_display() {
        let args = Args::default();

        let err = PendingWorkCommand::from_args_typed(&args).unwrap_err();

        assert!(matches!(err, PendingWorkError::MissingSubcommand));
        assert_eq!(err.to_string(), "a pw subcommand is required.");
    }

    #[test]
    fn unknown_action_returns_typed_error_with_legacy_display() {
        let args = Args {
            action: Some("nope".to_string()),
            ..Args::default()
        };

        let err = PendingWorkCommand::from_args_typed(&args).unwrap_err();

        assert!(matches!(
            err,
            PendingWorkError::UnknownAction { ref action } if action == "nope"
        ));
        assert_eq!(err.to_string(), "Unknown action: nope");
    }
}
