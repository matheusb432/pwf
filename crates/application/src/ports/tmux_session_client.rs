pub trait TmuxSessionClient: Clone + Send + Sync + 'static {
    fn available(&self) -> bool;

    fn session_exists(&self, session: &str) -> Result<bool, String>;

    fn new_window_process_argv(
        &self,
        session: &str,
        repository: &str,
        window: &str,
        argv: &[String],
    ) -> Vec<String>;

    fn new_session_process_argv(&self, session: &str, repository: &str) -> Vec<String>;

    fn open_window(
        &self,
        session: &str,
        repository: &str,
        window: &str,
        argv: &[String],
    ) -> Result<(), String>;
}
