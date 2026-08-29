use std::{collections::BTreeMap, process::Command, sync::Arc};

/// Request-scoped environment used by session subprocess adapters.
#[derive(Clone)]
pub struct ProcessEnvironment(Arc<BTreeMap<String, String>>);

impl ProcessEnvironment {
    #[must_use]
    pub fn new(values: impl IntoIterator<Item = (String, String)>) -> Self {
        Self(Arc::new(values.into_iter().collect()))
    }

    #[must_use]
    pub fn inherited() -> Self {
        Self::new(
            std::env::vars_os().filter_map(|(key, value)| {
                Some((key.into_string().ok()?, value.into_string().ok()?))
            }),
        )
    }

    pub(super) fn command(&self, program: impl AsRef<std::ffi::OsStr>) -> Command {
        let mut command = Command::new(program);
        command.env_clear().envs(self.0.iter());
        command
    }
}

impl Default for ProcessEnvironment {
    fn default() -> Self {
        Self::inherited()
    }
}

impl std::fmt::Debug for ProcessEnvironment {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProcessEnvironment")
            .field("variables", &self.0.len())
            .finish_non_exhaustive()
    }
}
