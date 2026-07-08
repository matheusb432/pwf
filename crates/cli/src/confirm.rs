//! Interactive yes/no confirmation prompts. Detects whether stdin is a terminal
//! and, when it is, prompts on stderr with a default-aware `[y/N]`/`[Y/n]` hint
//! and reads the answer. Exposed as the `Confirm` trait so callers exercise the
//! yes / no / non-interactive paths without a real terminal.

use std::io::IsTerminal;

/// Which answer an empty Enter selects — and thus which letter the prompt
/// capitalises (`[Y/n]` for [`DefaultAnswer::Yes`], `[y/N]` for [`DefaultAnswer::No`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefaultAnswer {
    Yes,
    No,
}

/// Gate for a yes/no decision. The real impl prompts on a TTY; tests inject
/// deterministic answers.
pub trait Confirm {
    /// Is stdin a terminal we can prompt on? When false (agentic runs, pipes,
    /// CI), callers decide whether to proceed or refuse without prompting.
    fn interactive(&self) -> bool;
    /// Prompt `question` with a `default`-aware hint and report yes/no.
    fn confirm(&self, question: &str, default: DefaultAnswer) -> bool;
}

/// Interpret a raw answer line against `default`: an explicit `y`/`yes` or
/// `n`/`no` (case-insensitive) wins; anything else — including an empty Enter —
/// follows the default.
fn interpret(line: &str, default: DefaultAnswer) -> bool {
    let answer = line.trim().to_ascii_lowercase();
    match default {
        DefaultAnswer::Yes => !matches!(answer.as_str(), "n" | "no"),
        DefaultAnswer::No => matches!(answer.as_str(), "y" | "yes"),
    }
}

/// Real gate: prints the `default`-aware hint on stderr and reads a line from stdin.
pub struct RealConfirm;
impl Confirm for RealConfirm {
    fn interactive(&self) -> bool {
        std::io::stdin().is_terminal()
    }
    fn confirm(&self, question: &str, default: DefaultAnswer) -> bool {
        use std::io::Write;
        let hint = match default {
            DefaultAnswer::Yes => "[Y/n]",
            DefaultAnswer::No => "[y/N]",
        };
        eprint!("{question} {hint} ");
        let _ = std::io::stderr().flush();
        let mut line = String::new();
        // A read error (e.g. closed stdin) declines rather than act blindly.
        if std::io::stdin().read_line(&mut line).is_err() {
            return false;
        }
        interpret(&line, default)
    }
}

/// Fake gate for tests — including the integration suites in `tests/`, which
/// compile the lib without `cfg(test)`, so this stays ungated.
pub struct FakeConfirm {
    pub interactive: bool,
    pub answer: bool,
}
impl Confirm for FakeConfirm {
    fn interactive(&self) -> bool {
        self.interactive
    }
    fn confirm(&self, _question: &str, _default: DefaultAnswer) -> bool {
        self.answer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_yes_empty_enter_proceeds() {
        assert!(interpret("", DefaultAnswer::Yes));
        assert!(interpret("\n", DefaultAnswer::Yes));
        assert!(interpret("   ", DefaultAnswer::Yes));
    }

    #[test]
    fn default_yes_explicit_no_declines_case_insensitively() {
        assert!(!interpret("n", DefaultAnswer::Yes));
        assert!(!interpret("N", DefaultAnswer::Yes));
        assert!(!interpret("no", DefaultAnswer::Yes));
        assert!(!interpret("  NO  ", DefaultAnswer::Yes));
    }

    #[test]
    fn default_yes_anything_else_proceeds() {
        assert!(interpret("y", DefaultAnswer::Yes));
        assert!(interpret("yes", DefaultAnswer::Yes));
        assert!(interpret("maybe", DefaultAnswer::Yes));
    }

    #[test]
    fn default_no_empty_enter_declines() {
        assert!(!interpret("", DefaultAnswer::No));
        assert!(!interpret("\n", DefaultAnswer::No));
    }

    #[test]
    fn default_no_explicit_yes_proceeds_case_insensitively() {
        assert!(interpret("y", DefaultAnswer::No));
        assert!(interpret("YES", DefaultAnswer::No));
    }

    #[test]
    fn default_no_anything_else_declines() {
        assert!(!interpret("maybe", DefaultAnswer::No));
        assert!(!interpret("x", DefaultAnswer::No));
    }
}
