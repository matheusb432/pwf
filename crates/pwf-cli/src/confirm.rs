//! Separates confirmation-answer interpretation from terminal I/O.

use std::io::Write;

/// Reports the result of a confirmation request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Confirmation {
    /// The operator accepted the requested action.
    Accepted,
    /// The operator declined the requested action or stdin could not be read.
    Declined,
    /// Stdin is not a terminal, so no question was presented.
    NonInteractive,
}

/// Interprets `no` case-insensitively; all other input accepts the default.
fn interpret(line: &str) -> Confirmation {
    let answer = line.trim().to_ascii_lowercase();
    let accepted = !matches!(answer.as_str(), "n" | "no");
    if accepted {
        Confirmation::Accepted
    } else {
        Confirmation::Declined
    }
}

/// Asks the question on stderr and reads the answer from terminal stdin.
///
/// Callers establish interactivity first via [`crate::console::Console`].
pub(crate) fn prompt(question: &str) -> Confirmation {
    eprint!("{question} [Y/n] ");
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    // A read failure declines the action.
    if std::io::stdin().read_line(&mut line).is_err() {
        Confirmation::Declined
    } else {
        interpret(&line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_yes_empty_enter_proceeds() {
        assert_eq!(interpret(""), Confirmation::Accepted);
        assert_eq!(interpret("\n"), Confirmation::Accepted);
        assert_eq!(interpret("   "), Confirmation::Accepted);
    }

    #[test]
    fn default_yes_explicit_no_declines_case_insensitively() {
        for answer in ["n", "N", "no", "  NO  "] {
            assert_eq!(interpret(answer), Confirmation::Declined);
        }
    }

    #[test]
    fn default_yes_anything_else_proceeds() {
        for answer in ["y", "yes", "maybe"] {
            assert_eq!(interpret(answer), Confirmation::Accepted);
        }
    }
}
