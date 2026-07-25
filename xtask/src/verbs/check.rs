//! Aggregate static check verb.

use std::time::Duration;

use anyhow::Result;

use super::{format, lint, test};
use crate::{
    process::{self, Status},
    task::{self, Step},
    verb::Verb,
};

const SQLX_PREPARE_CHECK_DEADLINE: Duration = Duration::from_mins(20);

pub(crate) fn run() -> Result<()> {
    task::check_all(
        &steps(format::check_steps()?),
        "run `just fix` / `just fmt`",
    )?;
    process::result(Verb::CHECK, Status::Pass);
    Ok(())
}

fn steps(mut formatting: Vec<Step>) -> Vec<Step> {
    formatting.push(lint::check_step());
    formatting.push(test::ast_rules_scan_step());
    formatting.push(
        Step::new(
            "sqlx-prepare-check",
            "cargo",
            ["run", "--quiet", "-p", "xtask", "--", "prepare", "--check"],
        )
        .with_deadline(SQLX_PREPARE_CHECK_DEADLINE),
    );
    formatting
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregate_steps_append_sqlx_check_after_formatters_lint_and_ast_gate() {
        let formatting = vec![
            Step::new("rustfmt", "rustfmt", Vec::<&str>::new()),
            Step::new("rumdl", "rumdl", Vec::<&str>::new()),
        ];

        let steps = steps(formatting);
        let labels = steps.iter().map(Step::label).collect::<Vec<_>>();

        assert_eq!(
            labels,
            [
                "rustfmt",
                "rumdl",
                "clippy",
                "check-ast-rules",
                "sqlx-prepare-check"
            ]
        );
        let sqlx_prepare_check = steps
            .iter()
            .find(|step| step.label() == "sqlx-prepare-check")
            .expect("SQLx prepare check step");
        assert_eq!(
            sqlx_prepare_check.deadline(),
            Some(std::time::Duration::from_mins(20))
        );
    }
}
