//! Multiplexer driver seam. Dispatch orchestration depends only on the
//! provider-neutral [`MultiplexerDriver`] trait; each supported terminal
//! multiplexer is a sibling backend module (`zellij` today — tmux/screen slot
//! in the same way). A backend owns its binary name, CLI argument shapes, and
//! provider quirks, so adding one never touches dispatch logic.

mod zellij;

pub(in crate::engines::pending_work) use zellij::RealZellij;

/// Why adding the work tab to a multiplexer session failed.
#[derive(Debug)]
pub(in crate::engines::pending_work) enum NewTabError {
    /// The target session is not running (missing, or exited-and-resurrectable).
    SessionNotFound,
    /// Any other failure (stderr text carried for the error message).
    Other(String),
}

impl std::fmt::Display for NewTabError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NewTabError::SessionNotFound => write!(f, "session not found"),
            NewTabError::Other(m) => write!(f, "{m}"),
        }
    }
}

/// Side-effecting multiplexer operations, mockable in tests. One impl per
/// supported multiplexer; the orchestration is written against this trait so a
/// new backend is a sibling module, not an edit to dispatch.
pub(in crate::engines::pending_work) trait MultiplexerDriver {
    /// The multiplexer binary resolves and runs (preflight for absence errors).
    fn available(&self) -> bool;
    /// Add a tab named `tab` to running `session`, cwd `cwd`, running `argv`.
    fn new_tab(
        &self,
        session: &str,
        cwd: &str,
        tab: &str,
        argv: &[String],
    ) -> Result<(), NewTabError>;
    /// Create/resurrect `session` headlessly so a later `new_tab` can target it.
    fn ensure_session(&self, session: &str) -> Result<(), String>;
}

#[cfg(test)]
pub(in crate::engines::pending_work) mod fake {
    use std::cell::RefCell;

    use super::*;

    /// Scriptable driver. `new_tab_results` is consumed front-to-back per call.
    pub(in crate::engines::pending_work) struct FakeMux {
        pub available: bool,
        pub new_tab_results: RefCell<Vec<Result<(), NewTabError>>>,
        pub ensure_result: Result<(), String>,
        pub calls: RefCell<Vec<String>>,
    }

    impl FakeMux {
        pub fn new(available: bool, new_tab_results: Vec<Result<(), NewTabError>>) -> Self {
            Self {
                available,
                new_tab_results: RefCell::new(new_tab_results),
                ensure_result: Ok(()),
                calls: RefCell::new(vec![]),
            }
        }
    }

    impl MultiplexerDriver for FakeMux {
        fn available(&self) -> bool {
            self.available
        }
        fn new_tab(
            &self,
            session: &str,
            _cwd: &str,
            tab: &str,
            _argv: &[String],
        ) -> Result<(), NewTabError> {
            self.calls
                .borrow_mut()
                .push(format!("new_tab:{session}:{tab}"));
            self.new_tab_results.borrow_mut().remove(0)
        }
        fn ensure_session(&self, session: &str) -> Result<(), String> {
            self.calls.borrow_mut().push(format!("ensure:{session}"));
            self.ensure_result.clone()
        }
    }
}
