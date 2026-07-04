//! `handoff list` — print the current `LEDGER.md` verbatim.

use std::path::Path;

use crate::{
    cli::Args,
    engines::handoff::{errors::HandoffRead, paths::handoff_paths},
};

struct LedgerRead {
    exists: bool,
    content: HandoffRead<String>,
}

fn read_ledger_content(ledger: &Path) -> LedgerRead {
    let exists = ledger.exists();
    if !exists {
        return LedgerRead {
            exists,
            content: HandoffRead::complete(String::new()),
        };
    }
    match std::fs::read_to_string(ledger) {
        Ok(content) => LedgerRead {
            exists,
            content: HandoffRead::complete(content),
        },
        Err(_) => LedgerRead {
            exists,
            content: HandoffRead::degraded(String::new()),
        },
    }
}

pub(in crate::engines::handoff) fn invoke_list(root: &Path, _args: &Args) -> String {
    let paths = handoff_paths(root);
    let ledger = read_ledger_content(&paths.ledger);
    let content = ledger.content.value;
    if ledger.exists {
        content
    } else {
        "No active handoffs (LEDGER.md not found).".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::handoff::{errors::HandoffReadStatus, test_support::tempdir};

    #[test]
    fn read_ledger_reports_degraded_when_existing_ledger_is_unreadable() {
        let dir = tempdir();
        let ledger = dir.path().join("LEDGER.md");
        std::fs::create_dir(&ledger).unwrap();

        let read = read_ledger_content(&ledger);

        assert!(read.exists);
        assert_eq!(read.content.status, HandoffReadStatus::Degraded);
        assert_eq!(read.content.value, "");
    }
}
