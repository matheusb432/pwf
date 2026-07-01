// Top-level dispatcher for `pwf <verb>`. The `route` verb is delegated to the
// `pw` word-router in `route`; everything else dispatches here.

use super::{
    actions::{
        NewItemSpec, add_pending_work_item,
        list::{ListScope, OrderSpec},
        render_add_confirmation, run_cancel, run_check, run_list_action, run_remove, run_reopen,
        run_update,
    },
    agent::{probe::RealProbe, verify::verify_text_with_probe},
    color::use_color,
    continue_prompt::{continue_handoff_prompt, continue_plan_prompt},
    domain::commands::PendingWorkCommand,
    errors,
    launch::LaunchPolicy,
    model::Action,
    naming::stamp_date,
    new_add::NewAddInputs,
    query::{
        find_pending_item, load_config, resolve_managed_project_name_typed, resolve_project_repo,
    },
    route::run_route,
    section::Section,
};
use crate::{
    cli::Args,
    config::Config,
    engines::pending_work::actions::{run_resolve, run_show},
};

/// Top-level entry for a directly-parsed `pwf <verb>` command (main.rs only).
/// `Add`'s confirmation gets a presentation-only reformat here — never inside
/// `run_typed`, which `run_args` also calls for the handoff/check in-process
/// seams that parse `add`'s plain-text id out (see `add_render`'s doc comment).
pub fn run(command: &PendingWorkCommand) -> Result<String, String> {
    let out = run_typed(command).map_err(String::from)?;
    if matches!(command.action(), Action::Add) {
        Ok(render_add_confirmation(
            &out,
            use_color(command.args().color),
        ))
    } else {
        Ok(out)
    }
}

pub(in crate::engines::pending_work) fn run_typed(
    command: &PendingWorkCommand,
) -> Result<String, errors::PendingWorkError> {
    let args = command.args();

    let cfg = load_config(args)?;
    let date = stamp_date(&args.date);

    match command.action() {
        Action::Route => Ok(run_route(&cfg, args, &date)?),

        Action::Add => Ok(run_add(&cfg, args, &date)?),

        Action::List => {
            let only_project = if let Some(p) = args.project.as_deref() {
                Some(resolve_managed_project_name_typed(&cfg, p)?)
            } else {
                None
            };
            let scope = ListScope::from_flags(args.human, args.future, args.all)?;
            let order = OrderSpec::from_tokens(&args.order)?;
            Ok(run_list_action(
                &cfg,
                only_project.as_deref(),
                args.long,
                scope,
                args.number,
                args.effort,
                order,
            )?)
        }

        Action::Clean => {
            let only = if let Some(p) = args.project.as_deref() {
                Some(resolve_managed_project_name_typed(&cfg, p)?)
            } else {
                None
            };
            Ok(crate::engines::clean::run_clean_typed(
                &cfg,
                only.as_deref(),
                &date,
                args.dry_run,
                args.force,
                &crate::confirm::RealConfirm,
            )?)
        }

        Action::Verify => {
            let launcher = super::session::launcher_for(args.agent);
            let probe = RealProbe::resolve(launcher.binary());
            let item = if let Some(vid) = args.id.as_deref() {
                Some(find_pending_item(&cfg, vid)?)
            } else {
                None
            };
            let claude_model = item
                .as_ref()
                .and_then(|it| super::session::resolve_claude_model_for_verify(args.agent, it));
            Ok(verify_text_with_probe(
                item.as_ref(),
                launcher,
                &probe,
                claude_model,
            ))
        }

        Action::Check => Ok(run_check(&cfg, args)?),

        Action::Cancel => Ok(run_cancel(&cfg, args)?),

        Action::Reopen => Ok(run_reopen(&cfg, args)?),

        Action::Resolve => Ok(run_resolve(&cfg, args)?),

        Action::Show => Ok(run_show(&cfg, args)?),

        Action::Remove => Ok(run_remove(&cfg, args)?),

        Action::Update => Ok(run_update(&cfg, args)?),

        Action::Session => {
            let id = require_id(args, "session")?;
            // `-a`/`--append`: extend the body via the same path as `update -a`/
            // `--append` before dispatch, so the launch prompt carries the extension.
            if args.append.is_some() {
                run_update(&cfg, args)?;
            }
            Ok(super::session::dispatch(
                &cfg,
                id,
                super::session::DispatchOpts {
                    color: args.color,
                    assume_yes: args.assume_yes,
                    inline: args.inline,
                    launch: LaunchPolicy {
                        worktree: args.worktree.into(),
                        auto: args.auto.into(),
                    },
                    agent: args.agent,
                },
            )?)
        }
    }
}

pub fn run_args(args: &crate::cli::Args) -> Result<String, String> {
    run_args_typed(args).map_err(String::from)
}

pub(in crate::engines::pending_work) fn run_args_typed(
    args: &crate::cli::Args,
) -> Result<String, errors::PendingWorkError> {
    let command = PendingWorkCommand::from_args_typed(args)?;
    run_typed(&command)
}

/// Resolve the create section from `--section` (wins) or the `--human` shorthand.
fn resolve_add_section(args: &Args) -> Result<Option<Section>, errors::PendingWorkError> {
    if let Some(raw) = args.section.as_deref() {
        return Section::from_flag(raw).map(Some).ok_or_else(|| {
            errors::PendingWorkError::BadSection {
                value: raw.to_string(),
            }
        });
    }
    Ok(args.human.then_some(Section::Human))
}

pub(in crate::engines::pending_work) fn require_id<'args>(
    args: &'args Args,
    action: &'static str,
) -> Result<&'args str, errors::PendingWorkError> {
    args.id
        .as_deref()
        .ok_or(errors::PendingWorkError::MissingId { action })
}

/// `pwf add` — create a pending-work item. The prompt comes from positional
/// words, the repo's newest handoff (`--continue-handoff`), or a plan path
/// (`--continue <path>`); the clap layer makes those three mutually exclusive.
fn run_add(cfg: &Config, args: &Args, date: &str) -> Result<String, errors::PendingWorkError> {
    let section = resolve_add_section(args)?;
    let prereq = super::prereq::frontmatter_from_flags(cfg, &args.prereq)?;

    // Resolve (project_name, title, prompt) per the chosen prompt source.
    let (project_name, title, prompt): (String, Option<String>, String) = if args.continue_handoff {
        let raw = args
            .project
            .as_deref()
            .ok_or(errors::PendingWorkError::AddUsage)?;
        let (project_name, repo) = resolve_project_repo(cfg, raw)?;
        let (title, prompt) = continue_handoff_prompt(&repo)?;
        (project_name, Some(title), prompt)
    } else if let Some(path) = args.continue_path.as_deref() {
        let raw = args
            .project
            .as_deref()
            .ok_or(errors::PendingWorkError::AddUsage)?;
        let (project_name, _repo) = resolve_project_repo(cfg, raw)?;
        let (title, prompt) = continue_plan_prompt(&project_name, path);
        (project_name, Some(title), prompt)
    } else {
        let input = NewAddInputs::resolve(cfg, args)?;
        (
            input.project_name,
            Some(input.session),
            input.prompt.to_string(),
        )
    };

    add_pending_work_item(
        cfg,
        &NewItemSpec {
            project_name: &project_name,
            task_prompt: &prompt,
            task_title: title.as_deref(),
            created: date,
            section,
            prereq: prereq.as_deref(),
            effort: args.effort,
        },
    )
}

#[cfg(test)]
mod tests {
    use std::assert_matches;

    use super::*;

    fn command_for(action: Action) -> PendingWorkCommand {
        let stage = std::env::temp_dir().join(format!("pwf_run_{}", nanos()));
        std::fs::create_dir_all(&stage).unwrap();
        let config = stage.join("config.json");
        std::fs::write(
            &config,
            format!(
                r#"{{ "notesDir": "{}", "projects": {{ "pwf": "/repo" }}, "prefixes": {{ "pwf": "PWF" }} }}"#,
                stage.to_string_lossy().replace('\\', "\\\\")
            ),
        )
        .unwrap();
        PendingWorkCommand::new(
            action,
            Args {
                config_path: Some(config.to_string_lossy().into_owned()),
                ..Args::default()
            },
        )
    }

    fn nanos() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    }

    #[test]
    fn run_args_stringifies_missing_action_at_public_edge() {
        assert_eq!(
            run_args(&Args::default()).unwrap_err(),
            "a pw subcommand is required."
        );
    }

    #[test]
    fn run_args_stringifies_unknown_action_at_public_edge() {
        let args = Args {
            action: Some("nope".to_string()),
            ..Args::default()
        };

        assert_eq!(run_args(&args).unwrap_err(), "Unknown action: nope");
    }

    #[test]
    fn resolve_missing_id_returns_typed_error_with_legacy_display() {
        let command = command_for(Action::Resolve);
        let err = require_id(command.args(), "resolve").unwrap_err();

        assert_matches!(
            err,
            errors::PendingWorkError::MissingId { action } if action == "resolve"
        );
        assert_eq!(err.to_string(), "--id is required for resolve.");
    }

    #[test]
    fn section_flag_wins_over_human_shorthand() {
        let args = Args {
            section: Some("future".into()),
            human: true,
            ..Args::default()
        };
        assert_eq!(resolve_add_section(&args).unwrap(), Some(Section::Future));
    }

    #[test]
    fn human_shorthand_maps_to_human_section() {
        let args = Args {
            human: true,
            ..Args::default()
        };
        assert_eq!(resolve_add_section(&args).unwrap(), Some(Section::Human));
    }

    #[test]
    fn no_section_flags_means_general_area() {
        assert_eq!(resolve_add_section(&Args::default()).unwrap(), None);
    }

    #[test]
    fn invalid_section_value_errors() {
        let args = Args {
            section: Some("bogus".into()),
            ..Args::default()
        };

        let err = resolve_add_section(&args).unwrap_err();

        assert_matches!(
            err,
            errors::PendingWorkError::BadSection { ref value } if value == "bogus"
        );
        assert_eq!(
            err.to_string(),
            "Unknown --section value 'bogus'. Use one of: future, human, low-prio."
        );
    }
}
