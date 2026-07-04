//! `test` — pwf's test suite. Slim in-process unit/integration by default; `--e2e` runs the
//! binary suites (`cli_e2e`, `help_cli`); `--all` runs both. `--verbose` streams uncaptured logs.
//! Migrates the former `just pwf test` bash recipe.

use anyhow::Result;
use clap::{Args, ValueEnum};

use crate::{
    paths,
    proc::{self, Status},
    task::{self, Step},
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

/// The binary-e2e targets, excluded from the default `cargo test` (they carry `test = false`).
const E2E_TARGETS: &[&str] = &["--test", "cli_e2e", "--test", "help_cli"];

/// One `cargo test` invocation: terse (`--quiet`) unless `verbose`, which instead streams
/// uncaptured output (`-- --nocapture`). `targets` selects explicit test binaries (empty =
/// default).
fn cargo_test_step(label: &str, targets: &[&str], verbose: bool) -> Step {
    let mut step = Step::new(label, "cargo", ["test"]);
    if verbose {
        step = step
            .args(targets.iter().copied())
            .args(["--", "--nocapture"]);
    } else {
        step = step.args(["--quiet"]).args(targets.iter().copied());
    }
    step
}

/// The ordered `cargo test` steps for a scope. Pure — no I/O, so it is unit-tested directly.
fn plan(scope: Scope, verbose: bool) -> Vec<Step> {
    let unit = || cargo_test_step("test", &[], verbose);
    let e2e = || cargo_test_step("test:e2e", E2E_TARGETS, verbose);
    match scope {
        Scope::Unit => vec![unit()],
        Scope::E2e => vec![e2e()],
        Scope::All => vec![unit(), e2e()],
    }
}

/// Runs the selected scope. For e2e, the binary suites exercise `target/release/pwf`, so the
/// release binary is built first if absent.
pub(crate) fn run(scope: Scope, verbose: bool) -> Result<()> {
    if matches!(scope, Scope::E2e | Scope::All) {
        let bin = paths::repo_root()
            .join("target")
            .join("release")
            .join("pwf");
        if !bin.is_file() {
            proc::run("cargo build", "cargo", &["build", "--release"])?;
        }
    }
    task::run_all(&proc::ProcessRunner, &plan(scope, verbose))?;
    proc::result("test", Status::Pass);
    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    fn argv(step: &Step) -> Vec<&str> {
        step.argv().iter().map(String::as_str).collect()
    }

    #[test]
    fn unit_default_is_terse() {
        let steps = plan(Scope::Unit, false);
        assert_eq!(steps.len(), 1);
        assert_eq!(argv(&steps[0]), ["test", "--quiet"]);
    }

    #[test]
    fn verbose_drops_quiet_and_adds_nocapture() {
        let steps = plan(Scope::Unit, true);
        assert_eq!(argv(&steps[0]), ["test", "--", "--nocapture"]);
    }

    #[test]
    fn e2e_selects_the_binary_suites() {
        let steps = plan(Scope::E2e, false);
        assert_eq!(
            argv(&steps[0]),
            ["test", "--quiet", "--test", "cli_e2e", "--test", "help_cli"]
        );
    }

    #[test]
    fn all_runs_unit_then_e2e() {
        let steps = plan(Scope::All, false);
        assert_eq!(steps.len(), 2);
        assert_eq!(argv(&steps[0]), ["test", "--quiet"]);
        assert!(argv(&steps[1]).contains(&"cli_e2e"));
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
