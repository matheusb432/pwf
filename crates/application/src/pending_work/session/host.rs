//! Defines repository, inline-process, and Zellij capabilities for sessions.

pub trait InlineSessionClient: Clone + Send + Sync + 'static {
    fn run(&self, argv: &[String], repository: &str) -> Result<(), String>;
}

pub trait RepositorySessionClient: Clone + Send + Sync + 'static {
    fn is_directory(&self, path: &str) -> bool;
}

pub trait ZellijSessionClient: Clone + Send + Sync + 'static {
    fn available(&self) -> bool;

    fn new_tab_process_argv(
        &self,
        session: &str,
        repository: &str,
        tab: &str,
        argv: &[String],
    ) -> Vec<String>;

    fn open_tab(
        &self,
        session: &str,
        repository: &str,
        tab: &str,
        argv: &[String],
    ) -> Result<(), ZellijTabOpenError>;

    fn ensure_session(&self, session: &str) -> Result<(), String>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZellijTabOpenError {
    SessionNotFound,
    Rejected(String),
}
