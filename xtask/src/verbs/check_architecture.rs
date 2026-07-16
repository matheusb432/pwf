//! `check-architecture` — the mechanical containment gate. Loads the workspace's source trees
//! once and runs every check in [`crate::architecture_check`] over them; prints one
//! `<path>:<line>: <message>` line per violation on stderr and exits non-zero. Pure check logic
//! lives in `architecture_check`; this module is just the CLI-facing glue (reporting + exit
//! status), matching the split every other verb here uses over `task`/`proc`.

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

/// `<path>:<line>: <message>` — the gate's stderr line shape.
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
