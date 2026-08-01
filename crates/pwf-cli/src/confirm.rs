//! Separates confirmation-answer interpretation from terminal I/O.

use std::io::Write;

/// Selects the answer used for empty or unrecognized input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefaultAnswer {
    Yes,
    No,
}

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

/// Interprets `yes` and `no` case-insensitively; all other input follows `default`.
fn interpret(line: &str, default: DefaultAnswer) -> Confirmation {
    let answer = line.trim().to_ascii_lowercase();
    let accepted = match default {
        DefaultAnswer::Yes => !matches!(answer.as_str(), "n" | "no"),
        DefaultAnswer::No => matches!(answer.as_str(), "y" | "yes"),
    };
    if accepted {
        Confirmation::Accepted
    } else {
        Confirmation::Declined
    }
}

/// Asks the question on stderr and reads the answer from terminal stdin.
///
/// Callers establish interactivity first via [`crate::console::Console`].
pub(crate) fn prompt(question: &str, default: DefaultAnswer) -> Confirmation {
    let hint = match default {
        DefaultAnswer::Yes => "[Y/n]",
        DefaultAnswer::No => "[y/N]",
    };
    eprint!("{question} {hint} ");
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    // A read failure declines the action.
    if std::io::stdin().read_line(&mut line).is_err() {
        Confirmation::Declined
    } else {
        interpret(&line, default)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_yes_empty_enter_proceeds() {
        assert_eq!(interpret("", DefaultAnswer::Yes), Confirmation::Accepted);
        assert_eq!(interpret("\n", DefaultAnswer::Yes), Confirmation::Accepted);
        assert_eq!(interpret("   ", DefaultAnswer::Yes), Confirmation::Accepted);
    }

    #[test]
    fn default_yes_explicit_no_declines_case_insensitively() {
        for answer in ["n", "N", "no", "  NO  "] {
            assert_eq!(
                interpret(answer, DefaultAnswer::Yes),
                Confirmation::Declined
            );
        }
    }

    #[test]
    fn default_yes_anything_else_proceeds() {
        for answer in ["y", "yes", "maybe"] {
            assert_eq!(
                interpret(answer, DefaultAnswer::Yes),
                Confirmation::Accepted
            );
        }
    }

    #[test]
    fn default_no_empty_enter_declines() {
        assert_eq!(interpret("", DefaultAnswer::No), Confirmation::Declined);
        assert_eq!(interpret("\n", DefaultAnswer::No), Confirmation::Declined);
    }

    #[test]
    fn default_no_explicit_yes_proceeds_case_insensitively() {
        for answer in ["y", "YES"] {
            assert_eq!(interpret(answer, DefaultAnswer::No), Confirmation::Accepted);
        }
    }

    #[test]
    fn default_no_anything_else_declines() {
        for answer in ["maybe", "x"] {
            assert_eq!(interpret(answer, DefaultAnswer::No), Confirmation::Declined);
        }
    }
}
