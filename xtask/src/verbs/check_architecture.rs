//! CLI reporting for the mechanical architecture gates.
//!
//! Violations use `<path>:<line>: <message>` on stderr and produce a non-zero exit.

use anyhow::{Result, bail};

use crate::{
    architecture_check::{self, Violation},
    paths,
    proc::{self, Status},
};

pub(crate) fn run() -> Result<()> {
    match architecture_check::run(&paths::repo_root())? {
        Ok(()) => {
            proc::result("check-architecture", Status::Pass);
            Ok(())
        }
        Err(violations) => {
            for line in violations.iter().map(format_violation) {
                eprintln!("{line}");
            }
            bail!("check-architecture found {} violation(s)", violations.len());
        }
    }
}

/// Formats the gate's stable `<path>:<line>: <message>` diagnostic.
fn format_violation(violation: &Violation) -> String {
    format!(
        "{}:{}: {}",
        violation.relative_path, violation.line, violation.message
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_as_path_colon_line_colon_message() {
        let violation = Violation {
            relative_path: "crates/cli/src/engines/pending_work/actions/remove.rs".to_string(),
            line: 7,
            message: "pwf_infra outside composition root".to_string(),
        };
        assert_eq!(
            format_violation(&violation),
            "crates/cli/src/engines/pending_work/actions/remove.rs:7: pwf_infra outside composition root"
        );
    }
}
