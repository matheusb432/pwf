//! Test declarations, shared runner configuration, and ordered E2E worker.

use std::{ffi::OsString, time::Duration};

use anyhow::{Context, Result};
use clap::{Args, Subcommand, ValueEnum};
use xtk_test::{Run, Test, summary};

use crate::{process, task::Step};

const E2E_TIMEOUT: Duration = Duration::from_hours(1);

#[expect(
    clippy::struct_excessive_bools,
    reason = "CLI flags map directly to Clap arguments"
)]
#[derive(Args)]
#[command(args_conflicts_with_subcommands = true)]
pub(crate) struct TestArgs {
    #[command(subcommand)]
    command: Option<TestCommand>,
    /// Stream full test output live; the log still captures it.
    #[arg(long)]
    pub(crate) verbose: bool,
    /// Emit one JSON report to stdout.
    #[arg(long)]
    pub(crate) json: bool,
    /// Provide an evidence directory to E2E tests and save report.json.
    #[arg(long)]
    pub(crate) evidences: bool,
    /// Which part of the suite to run.
    #[arg(
        long,
        value_enum,
        default_value_t = Scope::Unit,
        default_value_ifs = [("e2e", "true", "e2e"), ("all", "true", "all")]
    )]
    pub(crate) scope: Scope,
    /// Shorthand for `--scope e2e`.
    #[arg(long, conflicts_with_all = ["all", "scope"])]
    e2e: bool,
    /// Shorthand for `--scope all`.
    #[arg(long, conflicts_with = "scope")]
    all: bool,
}

#[derive(Subcommand)]
enum TestCommand {
    /// Collect workspace test coverage with cargo-llvm-cov.
    Coverage(TestCoverageArguments),
}

#[derive(Args)]
#[command(disable_help_flag = true)]
struct TestCoverageArguments {
    /// Extra cargo-llvm-cov arguments; output defaults to --quiet when unspecified.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    arguments_extra: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub(crate) enum Scope {
    Unit,
    E2e,
    All,
}

pub(crate) fn run(arguments: &TestArgs) -> Result<()> {
    if let Some(TestCommand::Coverage(coverage)) = &arguments.command {
        return test_coverage(&coverage.arguments_extra);
    }

    run_scope(
        arguments.scope,
        arguments.verbose,
        arguments.json,
        arguments.evidences,
    )
}

pub(crate) fn run_all() -> Result<()> {
    run_scope(Scope::All, false, false, false)
}

fn run_scope(scope: Scope, verbose: bool, json: bool, evidences: bool) -> Result<()> {
    let executable = std::env::current_exe()
        .context("resolve the xtask executable")?
        .into_os_string();
    let declarations = selected_tests(scope, executable);

    Run::try_new(scope.to_string(), declarations)?
        .verbose(verbose)
        .json(json)
        .evidences_from_cargo_manifest(evidences, include_str!("../../Cargo.toml"))?
        .execute()?;

    Ok(())
}

fn test_coverage(arguments_extra: &[String]) -> Result<()> {
    process::run_step(&test_coverage_step(arguments_extra))
}

fn test_coverage_step(arguments_extra: &[String]) -> Step {
    let mut step = Step::new("test coverage", "cargo", ["llvm-cov", "--workspace"]);
    if !coverage_output_is_explicit(arguments_extra) {
        step = step.with_arguments(["--quiet"]);
    }
    step.with_arguments(arguments_extra.iter().cloned())
}

fn coverage_output_is_explicit(arguments: &[String]) -> bool {
    arguments
        .iter()
        .take_while(|argument| argument.as_str() != "--")
        .any(|argument| {
            matches!(argument.as_str(), "--quiet" | "--verbose")
                || argument.strip_prefix('-').is_some_and(|flags| {
                    !flags.is_empty() && flags.bytes().all(|flag| matches!(flag, b'q' | b'v'))
                })
        })
}

pub(crate) fn run_e2e_worker(verbose: bool) -> Result<()> {
    process::run(
        "release process build",
        "cargo",
        &["build", "--release", "-p", "pwf-cli", "-p", "pwf-migrator"],
    )?;
    let mut arguments = vec!["test", "-p", "pwf-e2e", "--test", "e2e"];
    if verbose {
        arguments.extend(["--", "--nocapture"]);
    } else {
        arguments.insert(1, "--quiet");
    }
    process::run("binary E2E suites", "cargo", &arguments)
}

fn selected_tests(scope: Scope, executable: OsString) -> Vec<xtk_test::Test> {
    match scope {
        Scope::Unit => tests_unit(),
        Scope::E2e => tests_e2e(executable),
        Scope::All => tests_all(executable),
    }
}

fn tests_unit() -> Vec<Test> {
    vec![
        Test::try_new("unit", "cargo")
            .expect("unit test definition is valid")
            .args(["test", "--quiet", "--workspace"])
            .verbose_arguments(["--", "--nocapture"])
            .summary_parser(summary::cargo),
    ]
}

fn tests_e2e(executable: OsString) -> Vec<Test> {
    vec![
        Test::try_new("e2e", executable)
            .expect("E2E test definition is valid")
            .arg("e2e-worker")
            .verbose_arguments(["--verbose"])
            .accepts_evidences()
            .timeout(E2E_TIMEOUT),
    ]
}

fn tests_all(executable: OsString) -> Vec<Test> {
    let mut tests = tests_unit();
    tests.extend(tests_e2e(executable.clone()));
    tests.extend([
        Test::try_new("architecture", executable)
            .expect("architecture test definition is valid")
            .arg("check-architecture"),
        Test::try_new("ast-rules", "ast-grep")
            .expect("AST rules test definition is valid")
            .args(["test", "--skip-snapshot-tests"]),
        Test::try_new("ast-scan", "ast-grep")
            .expect("AST scan test definition is valid")
            .args(["scan", "--globs", "!xtask/xtk_test/**"]),
    ]);
    tests
}

impl std::fmt::Display for Scope {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Unit => "unit",
            Self::E2e => "e2e",
            Self::All => "all",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{coverage_output_is_explicit, test_coverage_step};

    #[test]
    fn test_coverage_forwards_cargo_llvm_cov_arguments() {
        let step = test_coverage_step(&["--show-missing-lines".to_string()]);

        assert_eq!(step.label(), "test coverage");
        assert_eq!(step.program(), "cargo");
        assert_eq!(
            step.arguments(),
            ["llvm-cov", "--workspace", "--quiet", "--show-missing-lines"]
        );
    }

    #[test]
    fn test_coverage_preserves_explicit_output_options_before_test_arguments() {
        for option in ["-q", "-v", "-vv", "--quiet", "--verbose"] {
            assert!(coverage_output_is_explicit(&[option.to_string()]));
        }
        assert!(!coverage_output_is_explicit(&[
            "--".to_string(),
            "--verbose".to_string(),
        ]));
    }
}
