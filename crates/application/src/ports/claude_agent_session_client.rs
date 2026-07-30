use crate::pending_work::session::{AgentLaunch, AgentProbe};

pub trait ClaudeAgentSessionClient: Clone + Send + Sync + 'static {
    fn probe(&self) -> AgentProbe;

    fn preview(&self, launch: &AgentLaunch) -> Vec<String>;

    fn prepare(&self, launch: &AgentLaunch) -> Vec<String>;
}
