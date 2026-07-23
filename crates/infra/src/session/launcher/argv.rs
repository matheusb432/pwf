//! Builds argument-injection-safe agent launch vectors.

pub(super) struct LaunchArgv(Vec<String>);

impl LaunchArgv {
    pub(super) fn new(binary: &str) -> Self {
        Self(vec![binary.to_string()])
    }

    pub(super) fn flag(mut self, flag: &str, value: String) -> Self {
        self.0.push(flag.to_string());
        self.0.push(value);
        self
    }

    pub(super) fn positional(mut self, value: impl Into<String>) -> Self {
        self.0.push(value.into());
        self
    }

    pub(super) fn into_guarded(mut self, prompt: String) -> Vec<String> {
        self.0.push("--".to_string());
        self.0.push(prompt);
        self.0
    }
}
