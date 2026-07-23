//! Aggregate static check verb.

use anyhow::Result;

use super::{format, lint};
use crate::{
    process::{self, Status},
    task::{self, Step},
    verb::Verb,
};

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
    formatting
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregate_steps_append_lint_after_formatters() {
        let formatting = vec![
            Step::new("rustfmt", "rustfmt", Vec::<&str>::new()),
            Step::new("rumdl", "rumdl", Vec::<&str>::new()),
        ];

        let steps = steps(formatting);
        let labels = steps.iter().map(Step::label).collect::<Vec<_>>();

        assert_eq!(labels, ["rustfmt", "rumdl", "clippy"]);
    }
}
