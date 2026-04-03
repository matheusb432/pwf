// Top-level dispatcher for `pwf pw <verb>`. The `route` verb is delegated to the
// `pw` word-router in `route`; everything else dispatches here.

use super::actions::{run_check, run_list_action, run_remove, run_update};
use super::add::{NewItemSpec, add_pending_work_item};
use super::claude::{RealProbe, invoke_claude_launch, verify_json_with_probe};
use super::continue_prompt::{continue_handoff_prompt, continue_plan_prompt};
use super::errors;
use super::launch::{new_launch_spec, write_launch_spec};
use super::model::{Action, Item};
use super::naming::{project_index_path, stamp_date};
use super::new_add::NewAddInputs;
use super::query::{
    find_pending_item, load_config, resolve_managed_project_name, resolve_project_repo,
};
use super::route::run_route;
use super::section::Section;
use crate::cli::Args;
use crate::config::Config;

pub fn run(args: &crate::cli::Args) -> Result<String, String> {
    let action_raw = args
        .action
        .as_deref()
        .ok_or("a pw subcommand is required.")?
        .to_ascii_lowercase();

    let cfg = load_config(args)?;
    let date = stamp_date(&args.date);

    let action: Action = action_raw
        .parse()
        .map_err(|_| format!("Unknown action: {action_raw}"))?;

    match &action {
        Action::Route => run_route(&cfg, args, &date),

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
            let launch = new_launch_spec(&item, args.model.as_deref(), args.thinking.as_deref());
            Ok(write_launch_spec(
                &item,
                &launch,
                args.json,
                args.model.as_deref(),
                args.thinking.as_deref(),
            ))
        }

        Action::Add => run_add(&cfg, args, &date),

        Action::List => {
            let only_project = if let Some(p) = args.project.as_deref() {
                Some(resolve_managed_project_name(&cfg, p)?)
            } else {
                None
            };
            run_list_action(
                &cfg,
                only_project.as_deref(),
                args.json,
                args.long,
                args.future,
                args.human,
                args.number,
            )
        }

        Action::Clean => {
            let only = if let Some(p) = args.project.as_deref() {
                Some(resolve_managed_project_name(&cfg, p)?)
            } else {
                None
            };
            crate::engines::clean::run_clean(
                &cfg,
                only.as_deref(),
                &date,
                args.dry_run,
                args.json,
                args.force,
                &crate::engines::clean::RealConfirm,
            )
        }

        Action::Verify => {
            let probe = RealProbe::resolve();
            let item = if let Some(vid) = args.id.as_deref() {
                Some(find_pending_item(&cfg, vid)?)
            } else {
                None
            };
            Ok(verify_json_with_probe(item.as_ref(), &probe))
        }

        Action::LaunchClaude => {
            let id = args
                .id
                .as_deref()
                .ok_or("--id is required for launch-claude.")?;
            let probe = RealProbe::resolve();
            invoke_claude_launch(&cfg, id, &probe, args.force)
        }

        Action::Check => run_check(&cfg, args),

        Action::Resolve => {
            let id = args.id.as_deref().ok_or("--id is required for resolve.")?;
            let item = find_pending_item(&cfg, id)?;
            // ? File-model items carry the per-item note; legacy inline items only the index.
            let note_path = item.item_file.as_deref().unwrap_or(&item.note).to_string();
            if args.json {
                let obj = serde_json::json!({
                    "id": item.id,
                    "project": item.project,
                    "notePath": note_path,
                    "title": item.session,
                });
                Ok(serde_json::to_string_pretty(&obj).unwrap())
            } else {
                Ok(note_path)
            }
        }

        Action::Launch => {
            let id = args.id.as_deref().ok_or("--id is required for launch.")?;
            let item = find_pending_item(&cfg, id)?;
            if !item.launchable {
                return Err(errors::not_launchable(&item.id, &item.issues));
            }
            super::prereq::warn_unsatisfied_on_launch(&cfg, &item);
            let launch = new_launch_spec(&item, args.model.as_deref(), args.thinking.as_deref());
            Ok(write_launch_spec(
                &item,
                &launch,
                args.json,
                args.model.as_deref(),
                args.thinking.as_deref(),
            ))
        }

        Action::Remove => run_remove(&cfg, args),

        Action::Update => run_update(&cfg, args),
    }
}

/// Resolve the create section from `--section` (wins) or the `--human` shorthand.
fn resolve_add_section(args: &Args) -> Result<Option<Section>, String> {
    if let Some(raw) = args.section.as_deref() {
        return Section::from_flag(raw)
            .map(Some)
            .ok_or_else(|| errors::bad_section(raw));
    }
    Ok(args.human.then_some(Section::Human))
}

/// `pwf pw add` — create a pending-work item. The prompt comes from positional
/// words, the repo's newest handoff (`--continue-handoff`), or a plan path
/// (`--continue <path>`); the clap layer makes those three mutually exclusive.
fn run_add(cfg: &Config, args: &Args, date: &str) -> Result<String, String> {
    let section = resolve_add_section(args)?;
    let prereq = super::prereq::frontmatter_from_flags(cfg, &args.prereq)?;

    // Resolve (project_name, title, prompt) per the chosen prompt source.
    let (project_name, title, prompt): (String, Option<String>, String) = if args.continue_handoff {
        let raw = args.project.as_deref().ok_or(errors::ADD_HINT)?;
        let (project_name, repo) = resolve_project_repo(cfg, raw)?;
        let (title, prompt) = continue_handoff_prompt(&repo)?;
        (project_name, Some(title), prompt)
    } else if let Some(path) = args.continue_path.as_deref() {
        let raw = args.project.as_deref().ok_or(errors::ADD_HINT)?;
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
            json: args.json,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(
            resolve_add_section(&args).unwrap_err(),
            errors::bad_section("bogus")
        );
    }
}
