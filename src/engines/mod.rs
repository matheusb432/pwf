pub mod clean;
pub mod handoff;
pub mod migrate;
pub mod pending_work;

use std::str::FromStr;

use crate::cli::Args;

/// The CLI engine selector — the first positional token (`pwf <engine> …`).
#[derive(Debug, Clone, PartialEq)]
pub enum Engine {
    PendingWork,
    Handoff,
    Migrate,
}

const PW: &str = "pw";
const PENDING_WORK: &str = "pending-work"; // back-compat alias of `pw`
const HANDOFF: &str = "handoff";
const MIGRATE: &str = "migrate";

impl Engine {
    /// Canonical engine name (`pw`, never the `pending-work` alias).
    pub fn as_str(&self) -> &'static str {
        match self {
            Engine::PendingWork => PW,
            Engine::Handoff => HANDOFF,
            Engine::Migrate => MIGRATE,
        }
    }
}

impl FromStr for Engine {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            PW | PENDING_WORK => Ok(Engine::PendingWork),
            HANDOFF => Ok(Engine::Handoff),
            MIGRATE => Ok(Engine::Migrate),
            _ => Err(()),
        }
    }
}

/// Dispatch one engine. Returns stdout text to print; engine writes files itself.
///
/// # Errors
///
/// Returns an error string if `engine` is unknown or the selected engine fails.
pub fn run(engine: &str, args: &Args) -> Result<String, String> {
    match engine.parse::<Engine>() {
        Ok(Engine::PendingWork) => pending_work::run_args(args),
        Ok(Engine::Handoff) => handoff::run(args),
        Ok(Engine::Migrate) => migrate::run(args),
        Err(()) => Err(format!("unknown engine: {engine}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // `pw` is the canonical engine name; `pending-work` is the kept alias. Both
    // parse to the same variant, and `as_str` normalizes back to the canonical.
    #[test]
    fn pw_is_canonical_pending_work_is_alias() {
        assert_eq!("pw".parse::<Engine>(), Ok(Engine::PendingWork));
        assert_eq!("pending-work".parse::<Engine>(), Ok(Engine::PendingWork));
        assert_eq!(Engine::PendingWork.as_str(), "pw");
    }

    #[test]
    fn unknown_engine_does_not_parse() {
        assert_eq!("bogus".parse::<Engine>(), Err(()));
    }

    // Behavioral: both names route to the same handler (action check precedes
    // config I/O in pending_work::run, so Args::default() exercises routing).
    #[test]
    fn pw_aliases_pending_work_in_run() {
        let args = Args::default();
        assert_eq!(run("pw", &args), run("pending-work", &args));
        assert_eq!(
            run("pw", &args),
            Err("a pw subcommand is required.".to_string())
        );
    }

    #[test]
    fn unknown_engine_is_rejected_by_run() {
        assert_eq!(
            run("bogus", &Args::default()),
            Err("unknown engine: bogus".to_string())
        );
    }
}
