//! Test declarations, shared runner configuration, and ordered E2E worker.

use std::{ffi::OsString, time::Duration};

use anyhow::{Context, Result};
use clap::{Args, Subcommand, ValueEnum};
use xtk_test::{OutputPath, Run, Test, TestCountDiscovery, surface};

use crate::{process, task::Step};

const E2E_TIMEOUT: Duration = Duration::from_hours(1);

#[derive(Args)]
#[command(args_conflicts_with_subcommands = true)]
pub(crate) struct TestArgs {
    #[command(subcommand)]
    command: Option<TestCommand>,
    #[command(flatten)]
    output: TestOutputArguments,
    #[command(flatten)]
    selection: TestSelectionArguments,
}

#[derive(Args, Clone, Copy, Default)]
struct TestOutputArguments {
    /// Stream full test output live; the log still captures it.
    #[arg(long)]
    verbose: bool,
    /// Emit one JSON report to stdout.
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct TestSelectionArguments {
    /// Which part of the suite to run.
    #[arg(
        long,
        value_enum,
        default_value_t = Scope::Unit,
        default_value_ifs = [("e2e", "true", "e2e"), ("all", "true", "all")]
    )]
    scope: Scope,
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

    run_scope(arguments.selection.scope, arguments.output)
}

pub(crate) fn run_all() -> Result<()> {
    run_scope(Scope::All, TestOutputArguments::default())
}

fn run_scope(scope: Scope, output: TestOutputArguments) -> Result<()> {
    let executable = std::env::current_exe()
        .context("resolve the xtask executable")?
        .into_os_string();
    let declarations = selected_tests(scope, executable)?;

    Run::try_new(scope.to_string(), declarations)?
        .verbose(output.verbose)
        .json(output.json)
        .output_path(OutputPath::default())
        .execute()?;

    Ok(())
}

fn test_coverage(arguments_extra: &[String]) -> Result<()> {
    process::run_step(&test_coverage_step(arguments_extra))?;
    if coverage_cleanup_is_required(arguments_extra) {
        process::run_step(&Step::new(
            "clean coverage artifacts",
            "cargo",
            ["clean", "--target-dir", "target/llvm-cov-target"],
        ))?;
    }
    Ok(())
}

fn test_coverage_step(arguments_extra: &[String]) -> Step {
    let mut step = Step::new("test coverage", "cargo", ["llvm-cov", "--workspace"]);
    if !coverage_output_is_explicit(arguments_extra) {
        step = step.with_arguments(["--quiet"]);
    }
    step.with_arguments(arguments_extra.iter().cloned())
}

fn coverage_cleanup_is_required(arguments: &[String]) -> bool {
    !arguments
        .iter()
        .take_while(|argument| argument.as_str() != "--")
        .any(|argument| matches!(argument.as_str(), "-h" | "--help" | "--no-report"))
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

fn selected_tests(scope: Scope, executable: OsString) -> Result<Vec<xtk_test::Test>> {
    match scope {
        Scope::Unit => tests_unit(),
        Scope::E2e => tests_e2e(executable),
        Scope::All => tests_all(executable),
    }
}

fn tests_unit() -> Result<Vec<Test>> {
    Ok(vec![
        Test::try_new("unit", surface::CARGO, "cargo")?
            .args(["test", "--quiet", "--workspace"])
            .test_count_discovery(TestCountDiscovery::CARGO_TEST_HARNESS)
            .verbose_arguments(["--", "--nocapture"]),
    ])
}

fn tests_e2e(executable: OsString) -> Result<Vec<Test>> {
    Ok(vec![
        Test::try_new("e2e", surface::OPAQUE, executable)?
            .arg("e2e-worker")
            .verbose_arguments(["--verbose"])
            .timeout(E2E_TIMEOUT),
    ])
}

fn tests_all(executable: OsString) -> Result<Vec<Test>> {
    let mut tests = tests_unit()?;
    tests.extend(tests_e2e(executable.clone())?);
    tests.extend([
        Test::try_new("architecture", surface::OPAQUE, executable)?.arg("check-architecture"),
        Test::try_new("ast-rules", surface::OPAQUE, "ast-grep")?
            .args(["test", "--skip-snapshot-tests"]),
        Test::try_new("ast-scan", surface::OPAQUE, "ast-grep")?.args([
            "scan",
            "--globs",
            "!xtask/xtk_test/**",
        ]),
    ]);
    Ok(tests)
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
    use super::{coverage_cleanup_is_required, coverage_output_is_explicit, test_coverage_step};

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

    #[test]
    fn test_coverage_cleanup_requires_a_generated_report() {
        assert!(!coverage_cleanup_is_required(&["--help".to_string()]));
        assert!(!coverage_cleanup_is_required(&["--no-report".to_string()]));
        assert!(coverage_cleanup_is_required(&[
            "--".to_string(),
            "--help".to_string(),
        ]));
    }
}
