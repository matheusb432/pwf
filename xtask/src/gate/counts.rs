//! Best-effort Cargo test count parsing.

use std::fmt::Write as _;

#[derive(Clone, Copy)]
pub(crate) enum Kind {
    Cargo,
    Plain,
}

pub(super) fn summarize(kind: Kind, output: &str) -> Option<String> {
    match kind {
        Kind::Cargo => cargo_counts(output),
        Kind::Plain => None,
    }
}

fn cargo_counts(output: &str) -> Option<String> {
    let mut passed = 0u64;
    let mut failed = 0u64;
    let mut seen = false;
    for line in output.lines() {
        let line = line.trim();
        if !line.starts_with("test result:") {
            continue;
        }
        seen = true;
        passed += count_before(line, "passed").unwrap_or(0);
        failed += count_before(line, "failed").unwrap_or(0);
    }
    seen.then(|| format_counts(passed, failed))
}

fn format_counts(passed: u64, failed: u64) -> String {
    let mut counts = format!("{passed} pass");
    if failed > 0 {
        let _ = write!(counts, ", {failed} fail");
    }
    counts
}

fn count_before(line: &str, keyword: &str) -> Option<u64> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    for (index, token) in tokens.iter().enumerate() {
        let clean = token.trim_end_matches([';', '.', ',']);
        if clean == keyword
            && index > 0
            && let Ok(count) = tokens[index - 1].parse::<u64>()
        {
            return Some(count);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cargo_counts_aggregate_across_test_binaries() {
        let output = "\
running 12 tests
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

running 72 tests
test result: ok. 72 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.44s
";
        assert_eq!(summarize(Kind::Cargo, output).as_deref(), Some("84 pass"));
    }

    #[test]
    fn cargo_counts_surface_failures() {
        let output =
            "test result: FAILED. 80 passed; 4 failed; 0 ignored; 0 measured; 0 filtered out\n";
        assert_eq!(
            summarize(Kind::Cargo, output).as_deref(),
            Some("80 pass, 4 fail")
        );
    }

    #[test]
    fn unparseable_output_degrades_to_none() {
        assert_eq!(summarize(Kind::Cargo, "no counts here\n"), None);
    }
}
