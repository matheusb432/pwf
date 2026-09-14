use clap::error::{ContextKind, ContextValue, ErrorKind};
use pwf_models::project::ProjectId;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("unknown managed project ID: {id}")]
    ProjectNotFound {
        id: ProjectId,
        suggestion: Option<ProjectId>,
    },
    #[error(transparent)]
    Runtime(#[from] anyhow::Error),
}

impl Error {
    pub(crate) fn exit(self, command: &mut clap::Command) -> ! {
        let (error, exit_code) = match self {
            Self::ProjectNotFound { id, suggestion } => {
                let usage = command.render_usage();
                let mut error = clap::Error::new(ErrorKind::InvalidValue).with_cmd(command);
                let argument = command
                    .get_arguments()
                    .find(|arg| arg.get_id() == "project");
                let argument =
                    argument.map_or_else(|| "<PROJECT>".to_string(), ToString::to_string);
                error.insert(ContextKind::InvalidArg, ContextValue::String(argument));
                error.insert(
                    ContextKind::InvalidValue,
                    ContextValue::String(id.as_ref().to_ascii_lowercase()),
                );
                if let Some(suggestion) = suggestion {
                    error.insert(
                        ContextKind::SuggestedValue,
                        ContextValue::String(suggestion.as_ref().to_ascii_lowercase()),
                    );
                }
                error.insert(ContextKind::Usage, ContextValue::StyledStr(usage));
                (error, 2)
            }
            Self::Runtime(error) => (command.error(ErrorKind::Io, format!("{error:#}")), 1),
        };
        let _ = error.print();
        std::process::exit(exit_code);
    }
}

pub(crate) fn rpc_error(error: pwf_client::ClientError) -> anyhow::Error {
    match error {
        pwf_client::ClientError::Rpc(status) => anyhow::anyhow!(status.message().to_string()),
        pwf_client::ClientError::InvalidTaskResponse(error) => anyhow::Error::new(error),
        pwf_client::ClientError::InvalidTaskDagResponse(error) => anyhow::Error::new(error),
    }
}

impl From<pwf_client::ClientError> for Error {
    fn from(error: pwf_client::ClientError) -> Self {
        Self::Runtime(rpc_error(error))
    }
}
