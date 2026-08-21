use clap::Args;
use pwf_client::{
    task::{Confirmation, ConfirmationPrompt, ConfirmedRequestError, TaskClient},
    v1::{
        Agent, AgentAvailability, BlockedByResolutionKind, DispatchMode, DispatchSessionOutcome,
        LaunchDirectives, PlanSessionIntent, PlanSessionRequest, SessionEffort, SessionWarning,
        TaskStatus, session_warning,
    },
};
use pwf_models::session::PushedPrompt;

use super::{
    AgentChoice, Identifier,
    render::{render_dispatch, render_dry_run, render_session_confirmation},
};
use crate::{
    confirmation::{ConfirmationAnswer, ConfirmationMode, prompt_error},
    console::Console,
};

#[derive(Args, Debug)]
pub struct Arguments {
    #[command(flatten)]
    pub(crate) identifier: Identifier,
    /// Color policy for the dispatch output
    #[arg(long, value_enum, default_value_t = ColorChoice::Auto)]
    pub(crate) color: ColorChoice,
    #[command(flatten)]
    confirmation: ConfirmationArguments,
    #[command(flatten)]
    execution: ExecutionArguments,
    #[command(flatten)]
    launch: LaunchDirectiveArguments,
    /// Which agent to dispatch
    #[arg(long = "agent", value_enum, default_value_t = AgentChoice::default())]
    pub(crate) agent: AgentChoice,
    /// Prefix text pushed to the agent prompt
    #[arg(short = 'p', long = "push-prompt", value_name = "TEXT")]
    pub(crate) pushed_prompt: Option<PushedPrompt>,
    /// Model override forwarded to the selected agent Wins over effort-tier resolution
    ///
    /// Use `default` or omit the flag to leave selection to effort-tier policy and provider
    /// configuration
    #[arg(long, short = 'm')]
    pub(crate) model: Option<String>,
    /// Reasoning effort for the dispatched agent session
    #[arg(long, value_enum, default_value_t = SessionEffortChoice::default())]
    pub(crate) effort: SessionEffortChoice,
}

#[derive(Args, Debug)]
struct ConfirmationArguments {
    /// Skip the dispatch confirmation (assume yes)
    #[arg(long = "yes", short = 'y')]
    assume_yes: bool,
}

#[derive(Args, Debug)]
struct ExecutionArguments {
    /// Run the agent inline in the current terminal
    #[arg(long = "inline", short = 'i')]
    inline: bool,
    /// Show the exact launch command without editing the task or starting anything
    #[arg(long, visible_alias = "dry")]
    dry_run: bool,
}

impl ExecutionArguments {
    fn intent(&self) -> PlanSessionIntent {
        if self.dry_run {
            PlanSessionIntent::DryRun
        } else {
            PlanSessionIntent::Dispatch
        }
    }

    fn mode(&self) -> DispatchMode {
        if self.inline {
            DispatchMode::Inline
        } else {
            DispatchMode::Multiplexer
        }
    }
}

#[derive(Args, Debug)]
struct LaunchDirectiveArguments {
    /// Instructs the agent to work in a git worktree named after the task id
    #[arg(long = "worktree", short = 'w')]
    worktree: bool,
    /// Append an autonomy directive so the agent runs without prompting the user (for
    /// unattended dispatch)
    #[arg(long = "auto")]
    autonomous: bool,
}

impl LaunchDirectiveArguments {
    fn directives(&self) -> LaunchDirectives {
        LaunchDirectives {
            worktree: self.worktree,
            autonomous: self.autonomous,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum ColorChoice {
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum SessionEffortChoice {
    Low,
    Medium,
    #[default]
    High,
    #[value(name = "xhigh")]
    XHigh,
    Max,
}

impl From<SessionEffortChoice> for SessionEffort {
    fn from(choice: SessionEffortChoice) -> Self {
        match choice {
            SessionEffortChoice::Low => Self::Low,
            SessionEffortChoice::Medium => Self::Medium,
            SessionEffortChoice::High => Self::High,
            SessionEffortChoice::XHigh => Self::Xhigh,
            SessionEffortChoice::Max => Self::Max,
        }
    }
}

pub(super) async fn run(
    arguments: &Arguments,
    console: Console,
    client: &TaskClient,
) -> anyhow::Result<String> {
    let agent = match arguments.agent {
        AgentChoice::Claude => Agent::Claude,
        AgentChoice::Codex => Agent::Codex,
    };
    let task_id = arguments
        .identifier
        .required(anyhow::anyhow!("--id is required for session."))?;
    let confirmation_mode = if arguments.execution.dry_run {
        ConfirmationMode::AssumeYes
    } else {
        console.confirmation_mode(arguments.confirmation.assume_yes)?
    };

    let request = PlanSessionRequest {
        task_id: task_id.to_string(),
        intent: arguments.execution.intent() as i32,
        pushed_prompt: arguments.pushed_prompt.as_ref().map(ToString::to_string),
        mode: arguments.execution.mode() as i32,
        directives: Some(arguments.launch.directives()),
        agent: agent as i32,
        model_override: arguments.model.clone(),
        effort: SessionEffort::from(arguments.effort) as i32,
        environment: process_environment(),
    };
    if arguments.execution.dry_run {
        let dry_run = client
            .plan_session(request)
            .await
            .map_err(crate::rpc_error)?;
        emit_session_warnings(&dry_run.warnings);
        if let Some(probe) = &dry_run.probe {
            render_probe(*probe);
        }
        return render_dry_run(&dry_run);
    }

    let prompt = SessionPrompt {
        console,
        mode: confirmation_mode,
    };
    let outcome = match client.dispatch_session(request, prompt).await {
        Ok(outcome) => outcome,
        Err(ConfirmedRequestError::Operation(status)) => {
            return Err(anyhow::anyhow!(status.message().to_string()));
        }
        Err(ConfirmedRequestError::Prompt(source)) => {
            return Err(prompt_error("session dispatch", source));
        }
    };
    if DispatchSessionOutcome::try_from(outcome.outcome).ok()
        == Some(DispatchSessionOutcome::InlineLaunch)
    {
        let launch = outcome.inline_launch.as_ref().ok_or_else(|| {
            anyhow::anyhow!("pwf-server returned an inline dispatch without a launch")
        })?;
        return execute_inline(launch);
    }
    render_dispatch(
        &outcome,
        console.color_with(match arguments.color {
            ColorChoice::Auto => None,
            ColorChoice::Always => Some(true),
            ColorChoice::Never => Some(false),
        }),
    )
}

fn process_environment() -> std::collections::HashMap<String, String> {
    std::env::vars_os()
        .filter_map(|(key, value)| Some((key.into_string().ok()?, value.into_string().ok()?)))
        .collect()
}

struct SessionPrompt {
    console: Console,
    mode: ConfirmationMode,
}

impl ConfirmationPrompt for SessionPrompt {
    type Error = dialoguer::Error;

    fn confirm(&self, confirmation: &Confirmation) -> Result<bool, Self::Error> {
        let Confirmation::DispatchSession(preflight) = confirmation else {
            return Ok(false);
        };
        emit_session_warnings(&preflight.warnings);
        if let Some(probe) = &preflight.probe {
            render_probe(*probe);
        }
        let Some(confirmation) = &preflight.confirmation else {
            return Ok(false);
        };
        if DispatchMode::try_from(confirmation.mode).ok() == Some(DispatchMode::Inline) {
            eprintln!("running {} inline...", confirmation.task_id);
        }
        match self.mode {
            ConfirmationMode::AssumeYes => Ok(true),
            ConfirmationMode::Prompt => self
                .console
                .confirm(&render_session_confirmation(confirmation))
                .map(|answer| answer == ConfirmationAnswer::Accepted),
        }
    }
}

fn render_probe(probe: pwf_client::v1::AgentProbe) {
    if AgentAvailability::try_from(probe.availability).ok() != Some(AgentAvailability::Available) {
        let binary = match Agent::try_from(probe.agent).ok() {
            Some(Agent::Claude) => "claude",
            Some(Agent::Codex) => "codex",
            Some(Agent::Unspecified) | None => "agent",
        };
        eprintln!(
            "note: {binary} not found on PATH from here; the agent will surface the error if it can't run."
        );
    }
}

fn emit_session_warnings(warnings: &[SessionWarning]) {
    if let Some(rendered) = render_session_warnings(warnings) {
        eprintln!("{rendered}");
    }
}

fn render_session_warnings(warnings: &[SessionWarning]) -> Option<String> {
    if warnings.is_empty() {
        return None;
    }
    let mut lines = vec!["warning: blocked_by information for this session:".to_string()];
    for warning in warnings {
        let line = match warning.value.as_ref() {
            Some(session_warning::Value::BlockedBy(blocked_by)) => {
                let title = blocked_by
                    .title
                    .as_deref()
                    .map(|title| format!(": {title}"))
                    .unwrap_or_default();
                match BlockedByResolutionKind::try_from(blocked_by.resolution).ok() {
                    Some(BlockedByResolutionKind::Found) => {
                        format!(
                            "  - {} ({}){title}",
                            blocked_by.id,
                            status_name(blocked_by.status)
                        )
                    }
                    Some(BlockedByResolutionKind::Missing) => {
                        format!("  - {} (missing; ignored as a blocker)", blocked_by.id)
                    }
                    Some(BlockedByResolutionKind::Unavailable) => format!(
                        "  - {} (unavailable: {})",
                        blocked_by.id,
                        blocked_by
                            .reason
                            .as_deref()
                            .unwrap_or("unknown")
                            .replace(['\r', '\n'], " ")
                    ),
                    Some(BlockedByResolutionKind::Unspecified) | None => {
                        format!("  - {} (unavailable: invalid status)", blocked_by.id)
                    }
                }
            }
            Some(session_warning::Value::BlockedByMetadata(issue)) => format!(
                "  - Malformed blocked_by metadata {:?} in {}: {}",
                issue.raw, issue.path, issue.reason
            ),
            None => "  - unavailable blocker diagnostic".to_string(),
        };
        lines.push(line);
    }
    lines.push(
        "session will continue; resolve active or cancelled blockers first when they still apply."
            .to_string(),
    );
    Some(lines.join("\n"))
}

fn status_name(value: Option<i32>) -> &'static str {
    match value.and_then(|value| TaskStatus::try_from(value).ok()) {
        Some(TaskStatus::Active) => "active",
        Some(TaskStatus::Done) => "done",
        Some(TaskStatus::Cancelled) => "cancelled",
        Some(TaskStatus::Unspecified) | None => "unspecified",
    }
}

#[cfg(unix)]
fn execute_inline(launch: &pwf_client::v1::InlineLaunch) -> anyhow::Result<String> {
    use std::os::unix::process::CommandExt as _;

    let (program, arguments) = launch
        .argv
        .split_first()
        .ok_or_else(|| anyhow::anyhow!("pwf-server returned an empty inline launch"))?;
    let error = std::process::Command::new(program)
        .args(arguments)
        .current_dir(&launch.working_directory)
        .exec();
    Err(anyhow::Error::new(error).context("executing the inline agent session"))
}

#[cfg(not(unix))]
fn execute_inline(launch: &pwf_client::v1::InlineLaunch) -> anyhow::Result<String> {
    let (program, arguments) = launch
        .argv
        .split_first()
        .ok_or_else(|| anyhow::anyhow!("pwf-server returned an empty inline launch"))?;
    let status = std::process::Command::new(program)
        .args(arguments)
        .current_dir(&launch.working_directory)
        .status()?;
    if !status.success() {
        return Err(anyhow::anyhow!("inline agent session exited with {status}"));
    }
    Ok(format!("# session {}: ran inline\n", launch.task_id))
}

#[cfg(test)]
mod tests {
    use pwf_client::v1::{
        BlockedByIssue, BlockedByResolutionKind, BlockedByStatus, SessionWarning, TaskStatus,
        session_warning,
    };

    use super::render_session_warnings;

    #[test]
    fn renders_all_session_blocker_diagnostics_as_one_informative_block() {
        let warnings = [
            SessionWarning {
                value: Some(session_warning::Value::BlockedBy(BlockedByStatus {
                    id: "AUX-0002".to_string(),
                    title: Some("prepare prior art".to_string()),
                    resolution: BlockedByResolutionKind::Found as i32,
                    status: Some(TaskStatus::Active as i32),
                    reason: None,
                })),
            },
            SessionWarning {
                value: Some(session_warning::Value::BlockedBy(BlockedByStatus {
                    id: "AUX-9999".to_string(),
                    title: None,
                    resolution: BlockedByResolutionKind::Missing as i32,
                    status: None,
                    reason: None,
                })),
            },
            SessionWarning {
                value: Some(session_warning::Value::BlockedByMetadata(BlockedByIssue {
                    path: "/tasks/PWF-0001.md".to_string(),
                    raw: "\"[[AUX-0001]]\"".to_string(),
                    reason: "expected a sequence".to_string(),
                })),
            },
        ];

        let rendered = render_session_warnings(&warnings).unwrap();

        assert_eq!(
            rendered,
            concat!(
                "warning: blocked_by information for this session:\n",
                "  - AUX-0002 (active): prepare prior art\n",
                "  - AUX-9999 (missing; ignored as a blocker)\n",
                "  - Malformed blocked_by metadata \"\\\"[[AUX-0001]]\\\"\" in /tasks/PWF-0001.md: expected a sequence\n",
                "session will continue; resolve active or cancelled blockers first when they still apply."
            )
        );
    }
}
