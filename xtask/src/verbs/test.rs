//! Test planning for workspace and binary suites.
//!
//! Unit and integration tests are the default. `--e2e` runs binary suites. `--all` runs both test
//! scopes plus the architecture and ast-grep source gates. Steps run through the captured gate,
//! which records full output to a log, prints a summary table, and emits the `RESULT` line.
//! `--verbose` also streams each step's output live.

use anyhow::Result;
use clap::{Args, ValueEnum};

use crate::{
    gate::{self, Job, Kind},
    paths, process,
    task::Step,
    verb::Verb,
};

/// Flags for the `test` verb. `--e2e`/`--all` are parse-time shorthands feeding `--scope`.
#[derive(Args)]
pub(crate) struct TestArgs {
    /// Stream full tool logs live instead of the terse default.
    #[arg(long)]
    pub(crate) verbose: bool,
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

/// Which part of the suite runs; clap parses `--scope` straight into this.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub(crate) enum Scope {
    Unit,
    E2e,
    All,
}

/// Binary suites excluded from default `cargo test` by `test = false`.
const E2E_TARGETS: &[&str] = &[
    "-p",
    "pwf",
    "--test",
    "cli_e2e",
    "--test",
    "help_cli",
    "--test",
    "project_cli",
];
const UNIT_TARGETS: &[&str] = &["--workspace"];

/// Builds one quiet or uncaptured `cargo test` step for selected targets.
fn cargo_test_step(label: &str, targets: &[&str], verbose: bool) -> Step {
    let mut step = Step::new(label, "cargo", ["test"]);
    if verbose {
        step = step
            .with_arguments(targets.iter().copied())
            .with_arguments(["--", "--nocapture"]);
    } else {
        step = step
            .with_arguments(["--quiet"])
            .with_arguments(targets.iter().copied());
    }
    step
}

/// Builds ordered test and gate steps for a scope, each paired with its count [`Kind`].
fn plan(scope: Scope, verbose: bool) -> Vec<(Step, Kind)> {
    let unit = || (cargo_test_step("test", UNIT_TARGETS, verbose), Kind::Cargo);
    let e2e = || {
        (
            cargo_test_step("test:e2e", E2E_TARGETS, verbose),
            Kind::Cargo,
        )
    };
    match scope {
        Scope::Unit => vec![unit()],
        Scope::E2e => vec![e2e()],
        Scope::All => vec![
            unit(),
            e2e(),
            (check_architecture_step(), Kind::Plain),
            (ast_rules_test_step(), Kind::Plain),
            (ast_rules_scan_step(), Kind::Plain),
        ],
    }
}

/// Builds the architecture-gate step used by the full suite.
fn check_architecture_step() -> Step {
    Step::new(
        "check-architecture",
        "cargo",
        ["run", "--quiet", "-p", "xtask", "--", "check-architecture"],
    )
}

/// Builds the step validating every `rules/` policy against its `rule-tests/` cases.
///
/// Snapshot comparison stays off: the cases assert only that valid snippets pass and invalid
/// snippets are flagged, not exact match spans.
fn ast_rules_test_step() -> Step {
    Step::new(
        "test:ast-rules",
        "ast-grep",
        ["test", "--skip-snapshot-tests"],
    )
}

/// Builds the ast-grep source gate enforcing every `rules/` policy on the tree.
pub(super) fn ast_rules_scan_step() -> Step {
    Step::new("check-ast-rules", "ast-grep", ["scan"])
}

/// Runs a test scope through the captured gate, building the release binary first when a binary
/// suite needs it.
pub(crate) fn run(scope: Scope, verbose: bool) -> Result<()> {
    if matches!(scope, Scope::E2e | Scope::All) {
        let bin = paths::repo_root()
            .join("target")
            .join("release")
            .join("pwf");
        if !bin.is_file() {
            process::run("cargo build", "cargo", &["build", "--release"])?;
        }
    }
    let jobs = plan(scope, verbose)
        .into_iter()
        .map(|(step, kind)| Job::new(step, kind))
        .collect::<Vec<_>>();
    gate::run(Verb::TEST.as_str(), &jobs, verbose)
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    fn argv(step: &Step) -> Vec<&str> {
        step.arguments().iter().map(String::as_str).collect()
    }

    #[test]
    fn unit_default_is_terse() {
        let steps = plan(Scope::Unit, false);
        assert_eq!(steps.len(), 1);
        assert_eq!(argv(&steps[0].0), ["test", "--quiet", "--workspace"]);
        assert!(matches!(steps[0].1, Kind::Cargo));
    }

    #[test]
    fn verbose_drops_quiet_and_adds_nocapture() {
        let steps = plan(Scope::Unit, true);
        assert_eq!(
            argv(&steps[0].0),
            ["test", "--workspace", "--", "--nocapture"]
        );
    }

    #[test]
    fn e2e_selects_the_binary_suites() {
        let steps = plan(Scope::E2e, false);
        assert_eq!(
            argv(&steps[0].0),
            [
                "test",
                "--quiet",
                "-p",
                "pwf",
                "--test",
                "cli_e2e",
                "--test",
                "help_cli",
                "--test",
                "project_cli"
            ]
        );
    }

    #[test]
    fn all_runs_unit_then_e2e_then_the_gates() {
        let steps = plan(Scope::All, false);
        assert_eq!(steps.len(), 5);
        assert_eq!(argv(&steps[0].0), ["test", "--quiet", "--workspace"]);
        assert!(argv(&steps[1].0).contains(&"cli_e2e"));
        assert_eq!(
            argv(&steps[2].0),
            ["run", "--quiet", "-p", "xtask", "--", "check-architecture"]
        );
        assert_eq!(argv(&steps[3].0), ["test", "--skip-snapshot-tests"]);
        assert_eq!(argv(&steps[4].0), ["scan"]);
        for (_, kind) in &steps[2..] {
            assert!(matches!(kind, Kind::Plain));
        }
    }

    #[test]
    fn gates_stay_out_of_the_slim_scopes() {
        for scope in [Scope::Unit, Scope::E2e] {
            let steps = plan(scope, false);
            assert_eq!(steps.len(), 1);
        }
    }

    #[derive(Parser)]
    struct Harness {
        #[command(flatten)]
        args: TestArgs,
    }

    #[test]
    fn shorthands_and_conflicts() {
        assert_eq!(
            Harness::try_parse_from(["t"]).unwrap().args.scope,
            Scope::Unit
        );
        assert_eq!(
            Harness::try_parse_from(["t", "--e2e"]).unwrap().args.scope,
            Scope::E2e
        );
        assert_eq!(
            Harness::try_parse_from(["t", "--all"]).unwrap().args.scope,
            Scope::All
        );
        assert!(Harness::try_parse_from(["t", "--e2e", "--all"]).is_err());
    }
}
