// Top-level dispatcher for `pwf <verb>`. The `route` verb is delegated to the
// `pw` word-router in `route`; everything else dispatches here.

use super::actions::list::ListScope;
use super::actions::{
    NewItemSpec, add_pending_work_item, run_cancel, run_check, run_list_action, run_remove,
    run_update,
};
use super::claude::{RealProbe, invoke_claude_launch, verify_text_with_probe};
use super::continue_prompt::{continue_handoff_prompt, continue_plan_prompt};
use super::domain::commands::PendingWorkCommand;
use super::errors;
use super::launch::write_launch_spec;
use super::model::{Action, Item};
use super::naming::{project_index_path, stamp_date};
use super::new_add::NewAddInputs;
use super::obsidian::store::ObsidianStore;
use super::query::{
    find_pending_item, load_config, resolve_managed_project_name_typed, resolve_project_repo,
};
use super::route::run_route;
use super::section::Section;
use crate::cli::Args;
use crate::config::Config;

// ? Frontmatter keys irrelevant to *executing* a task — dropped by `resolve --show`.
const SHOW_FRONTMATTER_DENYLIST: &[&str] = &["created"];

pub fn run(command: &PendingWorkCommand) -> Result<String, String> {
    run_typed(command).map_err(String::from)
}

pub(in crate::engines::pending_work) fn run_typed(
    command: &PendingWorkCommand,
) -> Result<String, errors::PendingWorkError> {
    let args = command.args();

    let cfg = load_config(args)?;
    let date = stamp_date(&args.date);

    match command.action() {
        Action::Route => Ok(run_route(&cfg, args, &date)?),

        Action::New => {
            // Build ad-hoc item + launch spec (no file writes).
            let input = NewAddInputs::resolve(&cfg, args, Action::New)?;
            let item = Item {
                id: format!("adhoc:{}", input.project_name),
                project: input.project_name.clone(),
                session: input.session.clone(),
                prompt: input.prompt.to_string(),
                repo: Some(input.repo.clone()),
                note: project_index_path(
                    cfg.notes_dir_for(&input.project_name),
                    &input.project_name,
                )
                .to_string_lossy()
                .into_owned(),
                item_file: None,
                line: 0,
                format: "adhoc".to_string(),
                marker_index: 0,
                marker_length: 0,
                launchable: true,
                needs_prompt: false,
                issues: vec![],
                section: None,
                prereq: None,
            };
            Ok(write_launch_spec(
                &item,
                args.model.as_deref(),
                args.thinking.as_deref(),
            ))
        }

        Action::Add => Ok(run_add(&cfg, args, &date)?),

        Action::List => {
            let only_project = if let Some(p) = args.project.as_deref() {
                Some(resolve_managed_project_name_typed(&cfg, p)?)
            } else {
                None
            };
            let scope = ListScope::from_flags(args.human, args.future, args.all)?;
            Ok(run_list_action(
                &cfg,
                only_project.as_deref(),
                args.long,
                scope,
                args.number,
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
                &crate::engines::clean::RealConfirm,
            )?)
        }

        Action::Verify => {
            let probe = RealProbe::resolve();
            let item = if let Some(vid) = args.id.as_deref() {
                Some(find_pending_item(&cfg, vid)?)
            } else {
                None
            };
            Ok(verify_text_with_probe(item.as_ref(), &probe))
        }

        Action::LaunchClaude => {
            let id = require_id(args, "launch-claude")?;
            let probe = RealProbe::resolve();
            Ok(invoke_claude_launch(&cfg, id, &probe, args.force)?)
        }

        Action::Check => Ok(run_check(&cfg, args)?),

        Action::Cancel => Ok(run_cancel(&cfg, args)?),

        Action::Resolve => {
            let id = require_id(args, "resolve")?;
            let item = find_pending_item(&cfg, id)?;
            // ? File-model items carry the per-item note; legacy inline items only the index.
            let note_path = item.item_file.as_deref().unwrap_or(&item.note).to_string();
            if args.show {
                // File-model: stream the note as markdown, minus exec-irrelevant frontmatter.
                // Legacy inline items have no standalone file → emit the parsed body.
                return match item.item_file.as_deref() {
                    Some(file) => {
                        let raw = ObsidianStore::read_item_file(std::path::Path::new(file))?;
                        Ok(crate::frontmatter::strip_frontmatter_keys(
                            &raw,
                            SHOW_FRONTMATTER_DENYLIST,
                        ))
                    }
                    None => Ok(item.prompt.clone()),
                };
            }
            Ok(note_path)
        }

        Action::Launch => {
            let id = require_id(args, "launch")?;
            let item = find_pending_item(&cfg, id)?;
            ensure_launchable(&item)?;
            super::prereq::warn_unsatisfied_on_launch(&cfg, &item);
            Ok(write_launch_spec(
                &item,
                args.model.as_deref(),
                args.thinking.as_deref(),
            ))
        }

        Action::Remove => Ok(run_remove(&cfg, args)?),

        Action::Update => Ok(run_update(&cfg, args)?),
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

fn ensure_launchable(item: &Item) -> Result<(), errors::PendingWorkError> {
    if item.launchable {
        return Ok(());
    }
    Err(errors::PendingWorkError::NotLaunchable {
        id: item.id.clone(),
        issues: item.issues.clone(),
    })
}

fn require_id<'args>(
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
        let input = NewAddInputs::resolve(cfg, args, Action::Add)?;
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
        },
    )
}

#[cfg(test)]
mod tests {
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
    fn launch_missing_id_returns_typed_error_with_legacy_display() {
        let command = command_for(Action::Launch);
        let err = require_id(command.args(), "launch").unwrap_err();

        assert!(matches!(
            err,
            errors::PendingWorkError::MissingId { action } if action == "launch"
        ));
        assert_eq!(err.to_string(), "--id is required for launch.");
        assert_eq!(
            run(&command_for(Action::Launch)).unwrap_err(),
            "--id is required for launch."
        );
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

        assert!(matches!(
            err,
            errors::PendingWorkError::MissingId { action } if action == "resolve"
        ));
        assert_eq!(err.to_string(), "--id is required for resolve.");
    }

    #[test]
    fn launch_claude_missing_id_returns_typed_error_with_legacy_display() {
        let command = command_for(Action::LaunchClaude);
        let err = require_id(command.args(), "launch-claude").unwrap_err();

        assert!(matches!(
            err,
            errors::PendingWorkError::MissingId { action } if action == "launch-claude"
        ));
        assert_eq!(err.to_string(), "--id is required for launch-claude.");
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

        assert!(matches!(
            err,
            errors::PendingWorkError::BadSection { ref value } if value == "bogus"
        ));
        assert_eq!(
            err.to_string(),
            "Unknown --section value 'bogus'. Use one of: future, human, low-prio."
        );
    }

    #[test]
    fn not_launchable_item_returns_typed_error_with_legacy_display() {
        let item = Item {
            id: "PWF-0001".to_string(),
            project: "pwf".to_string(),
            session: "blocked item".to_string(),
            prompt: "do work".to_string(),
            repo: Some("/repo/pwf".to_string()),
            note: "/notes/pwf/pwf.md".to_string(),
            item_file: None,
            line: 1,
            format: "index".to_string(),
            marker_index: 0,
            marker_length: 0,
            launchable: false,
            needs_prompt: true,
            issues: vec!["missing prompt".to_string(), "missing repo".to_string()],
            section: None,
            prereq: None,
        };

        let err = ensure_launchable(&item).unwrap_err();

        assert!(matches!(
            err,
            errors::PendingWorkError::NotLaunchable {
                ref id,
                ref issues
            } if id == "PWF-0001"
                && issues == &vec!["missing prompt".to_string(), "missing repo".to_string()]
        ));
        assert_eq!(
            err.to_string(),
            "Pending-work item 'PWF-0001' is not launchable: missing prompt; missing repo"
        );
    }
}
