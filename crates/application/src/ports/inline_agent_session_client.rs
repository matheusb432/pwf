pub trait InlineAgentSessionClient: Clone + Send + Sync + 'static {
    fn run(&self, argv: &[String], repository: &str) -> Result<(), String>;
}
