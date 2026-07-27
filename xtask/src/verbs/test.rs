//! Test runner and ordered E2E worker.

use std::ffi::OsString;

use anyhow::{Context, Result};
use clap::{Args, ValueEnum};
use xtk_test::Run;

use crate::{paths, process, project};

#[expect(
    clippy::struct_excessive_bools,
    reason = "CLI flags map directly to Clap arguments"
)]
#[derive(Args)]
pub(crate) struct TestArgs {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub(crate) enum Scope {
    Unit,
    E2e,
    All,
}

pub(crate) fn run(arguments: &TestArgs) -> Result<()> {
    let executable = std::env::current_exe()
        .context("resolve the xtask executable")?
        .into_os_string();
    let declarations = selected_tests(arguments.scope, executable);

    Run::new(
        arguments.scope.to_string(),
        declarations
            .into_iter()
            .map(project::TestDeclaration::into_test),
    )
    .verbose(arguments.verbose)
    .json(arguments.json)
    .evidences_from_cargo_manifest(arguments.evidences, include_str!("../../Cargo.toml"))?
    .execute()?;

    Ok(())
}

pub(crate) fn run_e2e_worker(verbose: bool) -> Result<()> {
    let binary = paths::repo_root()
        .join("target")
        .join("release")
        .join(format!("pwf{}", std::env::consts::EXE_SUFFIX));
    if !binary.is_file() {
        process::run("cargo build", "cargo", &["build", "--release"])?;
    }
    let mut arguments = vec![
        "test",
        "-p",
        "pwf",
        "--test",
        "cli_e2e",
        "--test",
        "project_cli",
    ];
    if verbose {
        arguments.extend(["--", "--nocapture"]);
    } else {
        arguments.insert(1, "--quiet");
    }
    process::run("binary E2E suites", "cargo", &arguments)
}

fn selected_tests(scope: Scope, executable: OsString) -> Vec<project::TestDeclaration> {
    match scope {
        Scope::Unit => project::tests_unit(),
        Scope::E2e => project::tests_e2e(executable),
        Scope::All => project::tests_all(executable),
    }
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
fn selected_test_labels(scope: Scope) -> Vec<&'static str> {
    selected_tests(scope, "xtask".into())
        .iter()
        .map(project::TestDeclaration::label)
        .collect()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use clap::Parser;

    use super::*;

    #[derive(Parser)]
    struct Harness {
        #[command(flatten)]
        args: TestArgs,
    }

    #[test]
    fn scopes_select_the_owned_declarations() {
        assert_eq!(selected_test_labels(Scope::Unit), ["unit"]);
        assert_eq!(selected_test_labels(Scope::E2e), ["e2e"]);
        assert_eq!(
            selected_test_labels(Scope::All),
            ["unit", "e2e", "architecture", "ast-rules", "ast-scan"]
        );
    }

    #[test]
    fn shorthands_flags_and_conflicts_parse_at_the_cli_boundary() {
        let arguments = Harness::try_parse_from(["t", "--e2e", "--json", "--evidences"])
            .unwrap()
            .args;
        assert_eq!(arguments.scope, Scope::E2e);
        assert!(arguments.json);
        assert!(arguments.evidences);
        assert!(Harness::try_parse_from(["t", "--e2e", "--all"]).is_err());
        assert!(Harness::try_parse_from(["t", "--scope", "unit", "--all"]).is_err());
    }

    #[test]
    fn e2e_timeout_exceeds_the_runner_default() {
        assert!(project::E2E_TIMEOUT > Duration::from_mins(30));
    }
}
