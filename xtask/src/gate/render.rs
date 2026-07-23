//! Deterministic summary and result rendering.

use std::{fmt::Write as _, time::Duration};

pub(super) struct Outcome {
    pub(super) label: String,
    pub(super) passed: bool,
    pub(super) summary: Option<String>,
    pub(super) elapsed: Duration,
}

pub(super) fn summary_table(scope: &str, outcomes: &[Outcome], color: bool) -> String {
    let label_width = outcomes
        .iter()
        .map(|outcome| outcome.label.len())
        .max()
        .unwrap_or(0)
        .max(5);

    let mut table = format!("## {} Summary\n", title_case(scope));
    for outcome in outcomes {
        let status = paint(verdict_word(outcome.passed), outcome.passed, color);
        let summary = outcome.summary.as_deref().unwrap_or("");
        let _ = writeln!(
            table,
            "{label:<label_width$}  {status}  {summary:<24}  {elapsed:>6}",
            label = outcome.label,
            elapsed = format_duration(outcome.elapsed),
        );
    }
    table
}

fn title_case(scope: &str) -> String {
    let mut characters = scope.chars();
    match characters.next() {
        Some(first) => first.to_uppercase().collect::<String>() + characters.as_str(),
        None => String::new(),
    }
}

pub(super) fn result_line(scope: &str, outcomes: &[Outcome], log: &str) -> String {
    let all_passed = outcomes.iter().all(|outcome| outcome.passed);
    let mut line = format!("RESULT scope={scope} status={}", status_word(all_passed));
    for outcome in outcomes {
        let _ = write!(line, " {}={}", outcome.label, status_word(outcome.passed));
    }
    let _ = write!(line, " log={log}");
    line
}

fn status_word(passed: bool) -> &'static str {
    if passed { "PASS" } else { "FAIL" }
}

fn verdict_word(passed: bool) -> &'static str {
    if passed { "Pass" } else { "Fail" }
}

fn paint(word: &str, passed: bool, color: bool) -> String {
    if !color {
        return word.to_string();
    }
    let code = if passed { "32" } else { "31" };
    format!("\x1b[{code}m{word}\x1b[0m")
}

fn format_duration(duration: Duration) -> String {
    format!("{:.1}s", duration.as_secs_f64())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(label: &str, passed: bool, summary: Option<&str>, seconds: f64) -> Outcome {
        Outcome {
            label: label.to_string(),
            passed,
            summary: summary.map(str::to_string),
            elapsed: Duration::from_secs_f64(seconds),
        }
    }

    fn passing_pair() -> Vec<Outcome> {
        vec![
            outcome("test", true, Some("84 pass"), 3.2),
            outcome("check-architecture", true, None, 8.5),
        ]
    }

    #[test]
    fn table_has_header_and_one_row_per_outcome() {
        let table = summary_table("test", &passing_pair(), false);
        assert!(table.starts_with("## Test Summary\n"));
        assert!(table.contains("test"));
        assert!(table.contains("check-architecture"));
        assert!(table.contains("84 pass"));
    }

    #[test]
    fn no_ansi_when_color_disabled() {
        let table = summary_table("test", &passing_pair(), false);
        assert!(!table.contains('\x1b'));
        assert!(table.contains("Pass"));
    }

    #[test]
    fn ansi_green_pass_when_color_enabled() {
        let table = summary_table("test", &passing_pair(), true);
        assert!(table.contains("\x1b[32mPass\x1b[0m"));
    }

    #[test]
    fn failing_row_renders_red_when_color_enabled() {
        let outcomes = [outcome("test", false, None, 1.0)];
        let table = summary_table("test", &outcomes, true);
        assert!(table.contains("\x1b[31mFail\x1b[0m"));
    }

    #[test]
    fn result_line_lists_every_field() {
        let line = result_line("test", &passing_pair(), "target/xtask/logs/test.log");
        assert_eq!(
            line,
            "RESULT scope=test status=PASS test=PASS check-architecture=PASS log=target/xtask/logs/test.log"
        );
    }

    #[test]
    fn result_line_fails_when_any_row_fails() {
        let outcomes = [
            outcome("test", false, None, 1.0),
            outcome("check-architecture", true, None, 1.0),
        ];
        let line = result_line("test", &outcomes, "log");
        assert_eq!(
            line,
            "RESULT scope=test status=FAIL test=FAIL check-architecture=PASS log=log"
        );
    }
}
