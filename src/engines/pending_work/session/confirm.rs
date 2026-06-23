//! Confirmation gate for `pwf session` dispatch: a default-yes `[Y/n]` prompt
//! shown before a real agent is launched, so an interactive operator can abort a
//! mistaken dispatch. Injectable so tests drive the yes / no / non-interactive
//! paths without a terminal.

use std::io::IsTerminal;

/// Decides whether a dispatch proceeds. The real impl prompts on a TTY; tests
/// inject deterministic answers.
pub(in crate::engines::pending_work) trait Confirm {
    /// Is stdin a terminal we can prompt on? When false (agentic dispatch, pipes,
    /// CI), the caller proceeds without prompting.
    fn interactive(&self) -> bool;
    /// Prompt `question` and report yes/no under default-yes semantics.
    fn confirm(&self, question: &str) -> bool;
}

/// Interpret a raw input line under default-yes semantics: an empty Enter or
/// anything but an explicit `n`/`no` (case-insensitive) proceeds.
fn is_yes(line: &str) -> bool {
    !matches!(line.trim().to_ascii_lowercase().as_str(), "n" | "no")
}

/// Real gate: prompts on stderr with `[Y/n]`, reads a line from stdin, and
/// defaults to yes on an empty answer.
pub(in crate::engines::pending_work) struct RealConfirm;
impl Confirm for RealConfirm {
    fn interactive(&self) -> bool {
        std::io::stdin().is_terminal()
    }
    fn confirm(&self, question: &str) -> bool {
        use std::io::Write;
        eprint!("{question} [Y/n] ");
        let _ = std::io::stderr().flush();
        let mut line = String::new();
        // A read error (e.g. closed stdin) declines rather than dispatch blindly.
        if std::io::stdin().read_line(&mut line).is_err() {
            return false;
        }
        is_yes(&line)
    }
}

/// Fake gate for tests.
#[cfg(test)]
pub(in crate::engines::pending_work) struct FakeConfirm {
    pub interactive: bool,
    pub answer: bool,
}
#[cfg(test)]
impl Confirm for FakeConfirm {
    fn interactive(&self) -> bool {
        self.interactive
    }
    fn confirm(&self, _question: &str) -> bool {
        self.answer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_enter_defaults_to_yes() {
        assert!(is_yes(""));
        assert!(is_yes("\n"));
        assert!(is_yes("   "));
    }

    #[test]
    fn explicit_no_declines_case_insensitively() {
        assert!(!is_yes("n"));
        assert!(!is_yes("N"));
        assert!(!is_yes("no"));
        assert!(!is_yes("NO"));
        assert!(!is_yes("  no  "));
    }

    #[test]
    fn yes_and_anything_else_proceed() {
        assert!(is_yes("y"));
        assert!(is_yes("yes"));
        assert!(is_yes("Y"));
        assert!(is_yes("maybe"));
    }
}
