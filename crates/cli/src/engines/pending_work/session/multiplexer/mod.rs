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
