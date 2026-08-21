use std::{ffi::OsString, process::Command, time::Duration};

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand, ValueEnum};
use xtk_test::{OutputPath, Run, Test, TestCountDiscovery, surface};

use crate::process;

const PROCESS_TIMEOUT: Duration = Duration::from_hours(1);

#[derive(Args)]
#[command(args_conflicts_with_subcommands = true)]
pub(crate) struct TestArgs {
    #[command(subcommand)]
    command: Option<TestCommand>,
    #[command(flatten)]
    output: TestOutputArguments,
    #[command(flatten)]
    selection: TestSelectionArguments,
    /// Cargo test arguments. The `all` scope rejects narrower native selectors.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    cargo_arguments: Vec<String>,
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
        default_value_ifs = [
            ("binary", "true", "binary"),
            ("e2e", "true", "e2e"),
            ("all", "true", "all")
        ]
    )]
    scope: Scope,
    /// Shorthand for `--scope binary`.
    #[arg(long, conflicts_with_all = ["e2e", "all", "scope"])]
    binary: bool,
    /// Shorthand for `--scope e2e`.
    #[arg(long, conflicts_with_all = ["binary", "all", "scope"])]
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
    Binary,
    E2e,
    All,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub(crate) enum ProcessScope {
    Binary,
    E2e,
    All,
}

pub(crate) fn run(arguments: &TestArgs) -> Result<()> {
    if let Some(TestCommand::Coverage(coverage)) = &arguments.command {
        return test_coverage(&coverage.arguments_extra);
    }

    run_scope(
        arguments.selection.scope,
        arguments.output,
        &arguments.cargo_arguments,
    )
}

pub(crate) fn run_all() -> Result<()> {
    run_scope(Scope::All, TestOutputArguments::default(), &[])
}

fn run_scope(scope: Scope, output: TestOutputArguments, cargo_arguments: &[String]) -> Result<()> {
    if scope == Scope::All && !cargo_arguments.is_empty() {
        bail!("cargo test arguments require the unit, binary, or e2e test scope");
    }

    let executable = std::env::current_exe()
        .context("resolve the xtask executable")?
        .into_os_string();
    let declarations = selected_tests(scope, executable, cargo_arguments, output.verbose)?;

    Run::try_new(scope.as_str(), declarations)?
        .verbose(output.verbose)
        .json(output.json)
        .output_path(OutputPath::default())
        .execute()?;

    Ok(())
}

fn test_coverage(arguments_extra: &[String]) -> Result<()> {
    process::run(
        "test coverage",
        Command::new("cargo").args(test_coverage_arguments(arguments_extra)),
    )?;
    if coverage_cleanup_is_required(arguments_extra) {
        process::run(
            "clean coverage artifacts",
            Command::new("cargo").args(["clean", "--target-dir", "target/llvm-cov-target"]),
        )?;
    }
    Ok(())
}

fn test_coverage_arguments(arguments_extra: &[String]) -> Vec<String> {
    let mut arguments = vec!["llvm-cov".to_owned()];
    if !cargo_package_scope_is_explicit(arguments_extra) {
        arguments.push("--workspace".to_owned());
    }
    if !cargo_output_is_explicit(arguments_extra) {
        arguments.push("--quiet".to_owned());
    }
    arguments.extend(arguments_extra.iter().cloned());
    arguments
}

fn coverage_cleanup_is_required(arguments: &[String]) -> bool {
    !arguments
        .iter()
        .take_while(|argument| argument.as_str() != "--")
        .any(|argument| matches!(argument.as_str(), "-h" | "--help" | "--no-report"))
}

fn cargo_output_is_explicit(arguments: &[String]) -> bool {
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

pub(crate) fn run_process_worker(
    scope: ProcessScope,
    verbose: bool,
    cargo_arguments: &[String],
) -> Result<()> {
    process::run(
        "release process build",
        Command::new("cargo").args([
            "build",
            "--release",
            "-p",
            "pwf-cli",
            "-p",
            "pwf-migrator",
            "-p",
            "pwf-server",
        ]),
    )?;
    process::run(
        scope.operation_name(),
        Command::new("cargo").args(process_test_arguments(scope, cargo_arguments, verbose)?),
    )
}

fn selected_tests(
    scope: Scope,
    executable: OsString,
    cargo_arguments: &[String],
    verbose: bool,
) -> Result<Vec<Test>> {
    match scope {
        Scope::Unit => tests_unit(cargo_arguments),
        Scope::Binary => tests_process(executable, ProcessScope::Binary, cargo_arguments, verbose),
        Scope::E2e => tests_process(executable, ProcessScope::E2e, cargo_arguments, verbose),
        Scope::All => tests_all(executable, verbose),
    }
}

fn tests_unit(cargo_arguments: &[String]) -> Result<Vec<Test>> {
    let emits_summary = cargo_test_emits_summary(cargo_arguments);
    let test = Test::try_new(
        "unit",
        if emits_summary {
            surface::CARGO
        } else {
            surface::OPAQUE
        },
        "cargo",
    )?
    .args(cargo_test_arguments(cargo_arguments));
    let test = if emits_summary && !cargo_test_harness_arguments_are_explicit(cargo_arguments) {
        test.test_count_discovery(TestCountDiscovery::CARGO_TEST_HARNESS)
            .verbose_arguments(["--", "--nocapture"])
    } else {
        test
    };
    Ok(vec![test])
}

fn tests_process(
    executable: OsString,
    scope: ProcessScope,
    cargo_arguments: &[String],
    verbose: bool,
) -> Result<Vec<Test>> {
    validate_process_test_arguments(cargo_arguments)?;
    let mut test = Test::try_new(scope.test_name(), surface::OPAQUE, executable)?
        .args(["process-worker", scope.as_str()]);
    if verbose {
        test = test.arg("--verbose");
    }
    if !cargo_arguments.is_empty() {
        test = test.arg("--").args(cargo_arguments.iter().cloned());
    }
    Ok(vec![test.timeout(PROCESS_TIMEOUT)])
}

fn tests_all(executable: OsString, verbose: bool) -> Result<Vec<Test>> {
    let mut tests = tests_unit(&[])?;
    tests.extend(tests_process(
        executable.clone(),
        ProcessScope::All,
        &[],
        verbose,
    )?);
    tests.extend([
        Test::try_new("architecture", surface::OPAQUE, executable)?.arg("check-architecture"),
        Test::try_new("ast-scan", surface::OPAQUE, "ast-grep")?.arg("scan"),
    ]);
    Ok(tests)
}

fn cargo_test_arguments(arguments_extra: &[String]) -> Vec<String> {
    let mut arguments = vec!["test".to_owned()];
    if !cargo_package_scope_is_explicit(arguments_extra) {
        arguments.push("--workspace".to_owned());
    }
    arguments.extend(arguments_extra.iter().cloned());
    arguments
}

fn process_test_arguments(
    scope: ProcessScope,
    arguments_extra: &[String],
    verbose: bool,
) -> Result<Vec<String>> {
    validate_process_test_arguments(arguments_extra)?;
    let mut arguments = vec!["test".to_owned()];
    if !verbose && !cargo_output_is_explicit(arguments_extra) {
        arguments.push("--quiet".to_owned());
    }
    arguments.extend(["-p", "pwf-cli"].into_iter().map(str::to_owned));
    for target in scope.target_names() {
        arguments.extend(["--test".to_owned(), (*target).to_owned()]);
    }
    arguments.extend(arguments_extra.iter().cloned());
    Ok(arguments)
}

fn validate_process_test_arguments(arguments: &[String]) -> Result<()> {
    if arguments
        .iter()
        .take_while(|argument| argument.as_str() != "--")
        .any(|argument| cargo_scope_option_is_explicit(argument))
    {
        bail!("the binary and e2e scopes fix the Cargo package and test target");
    }
    Ok(())
}

fn cargo_package_scope_is_explicit(arguments: &[String]) -> bool {
    arguments
        .iter()
        .take_while(|argument| argument.as_str() != "--")
        .any(|argument| cargo_package_scope_option_is_explicit(argument))
}

fn cargo_package_scope_option_is_explicit(argument: &str) -> bool {
    matches!(
        argument,
        "-p" | "--package" | "--workspace" | "--all" | "--manifest-path"
    ) || argument.starts_with("-p=")
        || argument.starts_with("--package=")
        || argument.starts_with("--manifest-path=")
}

fn cargo_scope_option_is_explicit(argument: &str) -> bool {
    cargo_package_scope_option_is_explicit(argument)
        || matches!(
            argument,
            "--exclude"
                | "--lib"
                | "--bin"
                | "--bins"
                | "--example"
                | "--examples"
                | "--test"
                | "--tests"
                | "--bench"
                | "--benches"
                | "--all-targets"
                | "--doc"
        )
        || ["--exclude=", "--bin=", "--example=", "--test=", "--bench="]
            .iter()
            .any(|prefix| argument.starts_with(prefix))
}

fn cargo_test_emits_summary(arguments: &[String]) -> bool {
    !arguments.iter().any(|argument| {
        matches!(
            argument.as_str(),
            "-h" | "--help" | "--list" | "--no-run" | "--version"
        )
    })
}

fn cargo_test_harness_arguments_are_explicit(arguments: &[String]) -> bool {
    arguments.iter().any(|argument| argument == "--")
}

impl Scope {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Unit => "unit",
            Self::Binary => "binary",
            Self::E2e => "e2e",
            Self::All => "all",
        }
    }
}

impl ProcessScope {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Binary => "binary",
            Self::E2e => "e2e",
            Self::All => "all",
        }
    }

    const fn test_name(self) -> &'static str {
        match self {
            Self::All => "process",
            scope => scope.as_str(),
        }
    }

    const fn operation_name(self) -> &'static str {
        match self {
            Self::Binary => "binary integration suite",
            Self::E2e => "E2E journey suite",
            Self::All => "binary integration and E2E suites",
        }
    }

    const fn target_names(self) -> &'static [&'static str] {
        match self {
            Self::Binary => &["binary"],
            Self::E2e => &["e2e"],
            Self::All => &["binary", "e2e"],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cargo_test_defaults_to_the_workspace() {
        assert_eq!(
            cargo_test_arguments(&["--no-run".to_owned()]),
            ["test", "--workspace", "--no-run"]
        );
    }

    #[test]
    fn cargo_test_preserves_a_package_scope() {
        assert_eq!(
            cargo_test_arguments(&[
                "--package".to_owned(),
                "xtask".to_owned(),
                "--no-run".to_owned(),
            ]),
            ["test", "--package", "xtask", "--no-run"]
        );
    }

    #[test]
    fn cargo_test_execution_only_modes_do_not_require_a_summary() {
        for argument in ["--help", "--list", "--no-run", "--version"] {
            assert!(!cargo_test_emits_summary(&[argument.to_owned()]));
        }
        assert!(cargo_test_emits_summary(&["selected_test".to_owned()]));
    }

    #[test]
    fn process_test_preserves_native_test_filters() {
        assert_eq!(
            process_test_arguments(
                ProcessScope::E2e,
                &[
                    "task::lists_tasks".to_owned(),
                    "--".to_owned(),
                    "--nocapture".to_owned(),
                ],
                false,
            )
            .unwrap(),
            [
                "test",
                "--quiet",
                "-p",
                "pwf-cli",
                "--test",
                "e2e",
                "task::lists_tasks",
                "--",
                "--nocapture",
            ]
        );
    }

    #[test]
    fn process_test_selects_binary_and_combined_targets() {
        assert_eq!(
            process_test_arguments(ProcessScope::Binary, &[], false).unwrap(),
            ["test", "--quiet", "-p", "pwf-cli", "--test", "binary"]
        );
        assert_eq!(
            process_test_arguments(ProcessScope::All, &[], false).unwrap(),
            [
                "test", "--quiet", "-p", "pwf-cli", "--test", "binary", "--test", "e2e",
            ]
        );
    }

    #[test]
    fn combined_process_scope_uses_the_clap_value_for_the_worker() {
        assert_eq!(ProcessScope::All.as_str(), "all");
        assert_eq!(ProcessScope::All.test_name(), "process");
    }

    #[test]
    fn process_test_rejects_a_conflicting_package_scope() {
        let error = process_test_arguments(
            ProcessScope::Binary,
            &["--package".to_owned(), "different-package".to_owned()],
            false,
        )
        .unwrap_err();

        assert!(error.to_string().contains("fix the Cargo package"));
    }

    #[test]
    fn test_coverage_defaults_to_workspace_and_quiet() {
        assert_eq!(
            test_coverage_arguments(&["--show-missing-lines".to_owned()]),
            ["llvm-cov", "--workspace", "--quiet", "--show-missing-lines",]
        );
    }

    #[test]
    fn test_coverage_preserves_a_package_scope() {
        assert_eq!(
            test_coverage_arguments(&["--package=xtask".to_owned()]),
            ["llvm-cov", "--quiet", "--package=xtask"]
        );
    }

    #[test]
    fn test_coverage_preserves_explicit_output_options_before_test_arguments() {
        for option in ["-q", "-v", "-vv", "--quiet", "--verbose"] {
            assert!(cargo_output_is_explicit(&[option.to_owned()]));
        }
        assert!(!cargo_output_is_explicit(&[
            "--".to_owned(),
            "--verbose".to_owned(),
        ]));
    }

    #[test]
    fn test_coverage_cleanup_requires_a_generated_report() {
        assert!(!coverage_cleanup_is_required(&["--help".to_owned()]));
        assert!(!coverage_cleanup_is_required(&["--no-report".to_owned()]));
        assert!(coverage_cleanup_is_required(&[
            "--".to_owned(),
            "--help".to_owned(),
        ]));
    }
}
