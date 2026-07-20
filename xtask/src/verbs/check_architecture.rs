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
            relative_path: "crates/domain/Cargo.toml".to_string(),
            line: 1,
            message: "pwf-domain must not depend on outward layer pwf-application".to_string(),
        };
        assert_eq!(
            format_violation(&violation),
            "crates/domain/Cargo.toml:1: pwf-domain must not depend on outward layer pwf-application"
        );
    }
}
