// Top-level dispatcher for `pwf <verb>`. The `route` verb is delegated to the
// `pw` word-router in `route`; everything else dispatches here.

use pwf_application::{
    AppDbStore, IndexEntry, IndexSection, NoteMarkdownSource, PendingWorkItem,
    pending_work::add::AddPendingWorkItem,
};
use pwf_domain::pending_work::{HANDOFF_TAG, MutationOutcome, ProjectName, ProjectRegistry};
use pwf_infra::obsidian::ObsidianStore;

use super::{
    actions::{
        AddedItem, EngineOutcome, ListParams, emit_created_section_diagnostic,
        emit_created_section_diagnostic_for_error,
        list::{ListScope, OrderSpec},
        render_outcome_confirmation, run_cancel, run_done, run_list_query, run_remove, run_reopen,
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
    engines::{
        handoff::mirror,
        pending_work::actions::{run_resolve, run_show},
    },
};

/// Top-level entry for a directly-parsed `pwf <verb>` command (main.rs only).
/// `Add`/`Remove`/`Update`'s confirmations get a presentation-only reformat
/// here — never inside `run_typed`, which `run_args` also calls for the
/// integration-test surface that drives pw verbs by `Args` directly, which
/// needs the plain, un-ANSI'd text (see `confirm_render`'s doc comment).
/// Handoff's in-process `add` seam reuses the same `AddPendingWorkItem`
/// request build + direct application operation as top-level `pwf add`, but
/// consumes the typed `AddedItem` instead of raw text; handoff no longer calls
/// `run_args` at all (PWF-0117 retired its `done`/`cancel`/`reopen`/`refresh`
/// verbs).
pub fn run(command: &PendingWorkCommand) -> Result<String, String> {
    let outcome = run_typed(command).map_err(String::from)?;
    let on = use_color(command.args().color);
    match render_outcome_confirmation(&outcome, on) {
        Some(rendered) => Ok(rendered),
        None => Ok(outcome.into_raw_text()),
    }
}

/// Build the single store instance an engine entry threads down to every
/// consumer for the rest of the call — the composition root this module owns
/// (PWF-0123 stage 1). `ObsidianStore` is constructed only here; every other
/// consumer takes the trait bounds it needs (the generic [`AppDbStore`] record
/// kinds, plus [`NoteMarkdownSource`] for the ghost-`show` storage-error
/// replay). The only other `pwf_infra` references in the CLI are the three
/// error-downcast exceptions (`query.rs`, `actions/add.rs`, `actions/done.rs`).
pub(crate) fn store_for(
    cfg: &Config,
) -> impl NoteMarkdownSource
+ AppDbStore<PendingWorkItem>
+ AppDbStore<IndexEntry>
+ AppDbStore<IndexSection>
+ use<> {
    ObsidianStore::new(cfg.clone())
}

/// Build the [`ProjectRegistry`] the read-side handlers use to route ids to
/// projects and to enumerate every managed project with its repo. Prefixes are
/// uppercased to match the canonical `WorkItemId` form.
pub(crate) fn project_registry(cfg: &Config) -> ProjectRegistry {
    ProjectRegistry::new(cfg.projects.iter().map(|(name, repo)| {
        (
            ProjectName::try_new(name).expect("configured project is non-empty"),
            Some(repo.clone()),
            cfg.prefixes
                .get(name)
                .map(|prefix| prefix.to_ascii_uppercase()),
        )
    }))
}

pub(in crate::engines::pending_work) fn run_typed(
    command: &PendingWorkCommand,
) -> Result<EngineOutcome, errors::PendingWorkError> {
    let args = command.args();

    let cfg = load_config(args)?;
    let store = store_for(&cfg);
    let date = stamp_date(args.date.as_deref());

    match command.action() {
        Action::Route => Ok(EngineOutcome::Text(run_route(&cfg, &store, args, &date)?)),

        Action::Add => Ok(EngineOutcome::Mutation(MutationOutcome::Added(run_add(
            &cfg, &store, args, &date,
        )?))),

        Action::List => run_list(&cfg, &store, args),

        Action::Clean => {
            let only = if let Some(p) = args.project.as_deref() {
                Some(resolve_managed_project_name_typed(&cfg, p)?)
            } else {
                None
            };
            Ok(EngineOutcome::Text(crate::engines::clean::run_clean_typed(
                &cfg,
                only.as_deref(),
                &date,
                args.dry_run,
                args.force,
                &crate::confirm::RealConfirm,
            )?))
        }

        Action::Verify => {
            let launcher = super::session::launcher_for(args.agent);
            let probe = RealProbe::resolve(launcher.binary());
            let item = if let Some(vid) = args.id.as_deref() {
                Some(find_pending_item(&store, &project_registry(&cfg), vid)?)
            } else {
                None
            };
            let claude_model = item.as_ref().and_then(|it| {
                super::session::resolve_model_for_verify(args.agent, it, args.model.as_deref())
            });
            Ok(EngineOutcome::Text(verify_text_with_probe(
                item.as_ref(),
                launcher,
                &probe,
                claude_model.as_ref(),
            )))
        }

        Action::Done => Ok(EngineOutcome::Text(run_done(&cfg, &store, args)?)),

        Action::Cancel => Ok(EngineOutcome::Text(run_cancel(&cfg, &store, args)?)),

        Action::Reopen => Ok(EngineOutcome::Text(run_reopen(&cfg, &store, args)?)),

        Action::Resolve => Ok(EngineOutcome::Text(run_resolve(&cfg, &store, args)?)),

        Action::Show => Ok(EngineOutcome::Text(run_show(&cfg, &store, args)?)),

        Action::Remove => run_remove(&cfg, &store, args, &crate::confirm::RealConfirm),

        Action::Update => Ok(EngineOutcome::Mutation(MutationOutcome::Updated(
            run_update(&store, &project_registry(&cfg), args)?,
        ))),

        Action::Session => {
            let id = require_id(args, "session")?;
            // `-a`/`--append`: extend the body via the same path as `update -a`/
            // `--append` before dispatch, so the launch prompt carries the extension.
            if args.append.is_some() {
                run_update(&store, &project_registry(&cfg), args)?;
            }
            Ok(EngineOutcome::Text(super::session::dispatch(
                &store,
                &project_registry(&cfg),
                id,
                &super::session::DispatchOpts {
                    color: args.color,
                    assume_yes: args.assume_yes,
                    inline: args.inline,
                    launch: LaunchPolicy {
                        worktree: args.worktree.into(),
                        auto: args.auto.into(),
                    },
                    agent: args.agent,
                    model_override: args.model.clone(),
                },
            )?))
        }
    }
}

fn run_list(
    cfg: &Config,
    store: &impl AppDbStore<PendingWorkItem>,
    args: &Args,
) -> Result<EngineOutcome, errors::PendingWorkError> {
    let only_project = if let Some(p) = args.project.as_deref() {
        Some(resolve_managed_project_name_typed(cfg, p)?)
    } else {
        None
    };
    let scope = ListScope::from_flags(args.human, args.future, args.all)?;
    let order = OrderSpec::from_tokens(&args.order)?;
    let tags = super::tags::from_flags(&args.tag)?;
    let params = ListParams {
        only_project: only_project.as_deref(),
        long: args.long,
        scope,
        number: args.number,
        effort: args.effort,
        tags: tags.as_ref(),
        order,
        color_on: use_color(args.color),
    };
    Ok(EngineOutcome::Text(run_list_query(cfg, store, params)?))
}

pub fn run_args(args: &crate::cli::Args) -> Result<String, String> {
    run_args_typed(args).map_err(String::from)
}

pub(in crate::engines::pending_work) fn run_args_typed(
    args: &crate::cli::Args,
) -> Result<String, errors::PendingWorkError> {
    let command = PendingWorkCommand::from_args_typed(args)?;
    run_typed(&command).map(EngineOutcome::into_raw_text)
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

/// Build the application command behind `pwf add` plus the store + registry to
/// execute it against, including the `--continue-handoff` / `--continue`
/// prompt sourcing and config/date resolution that cross-engine callers need
/// too. The store is built here (not by the caller) so `handoff`'s in-process
/// add seam (`pw_bridge::inprocess_pw_add`) never has to name `ObsidianStore`
/// itself — this function lives in an allowed composition-root file.
pub(crate) fn add_command_from_args(
    args: &Args,
) -> Result<
    (
        impl AppDbStore<PendingWorkItem> + AppDbStore<IndexEntry> + AppDbStore<IndexSection> + use<>,
        ProjectRegistry,
        AddPendingWorkItem,
    ),
    errors::PendingWorkError,
> {
    let cfg = load_config(args)?;
    let store = store_for(&cfg);
    let registry = project_registry(&cfg);
    let date = stamp_date(args.date.as_deref());
    let command = build_add_command(&cfg, args, &date)?;
    Ok((store, registry, command))
}

/// `pwf add` — create a pending-work item. The prompt comes from positional
/// words, the repo's newest handoff (`--continue-handoff`), or a plan path
/// (`--continue <path>`); the clap layer makes those three mutually exclusive.
fn run_add<S>(
    cfg: &Config,
    store: &S,
    args: &Args,
    date: &str,
) -> Result<AddedItem, errors::PendingWorkError>
where
    S: AppDbStore<PendingWorkItem> + AppDbStore<IndexEntry> + AppDbStore<IndexSection>,
{
    let command = build_add_command(cfg, args, date)?;

    // Preflight a handoff scaffold *before* the mutation when `--tag handoff`
    // is present, so a bad repo mapping or a path collision fails with no
    // item created. `--continue-handoff` is excluded: that flag means the pw
    // item continues an *existing* handoff, so it must never scaffold a new
    // one. `handoff add`'s in-process seam (`inprocess_pw_add`) never reaches
    // this function — it executes the built command directly through the
    // application operation — but the external `--pending-work-script`
    // allocator's canonical protocol (`pw_bridge::spawn_pw_add`) is `add --tag
    // handoff --continue-handoff`, and an operator's allocator script commonly
    // just execs the real `pwf` binary, which *does* land here.
    let scaffold = if !args.continue_handoff
        && command
            .tags
            .as_ref()
            .is_some_and(|tags| tags.contains_name(HANDOFF_TAG))
    {
        Some(mirror::preflight_scaffold(
            cfg,
            &command.project_name,
            command.title.as_deref().unwrap_or(""),
            date,
        )?)
    } else {
        None
    };

    let result =
        pwf_application::pending_work::add::execute(command, store, &project_registry(cfg));
    match result {
        Ok(added) => {
            emit_created_section_diagnostic(&added);
            if let Some(pending) = scaffold {
                let path = pending.commit(&added.id).map_err(|source| {
                    errors::PendingWorkError::HandoffMirrorAfterMutation {
                        id: added.id.clone(),
                        source,
                        remedy: errors::ADD_MIRROR_REMEDY.to_string(),
                    }
                })?;
                eprintln!("info: created handoff {}", path.display());
            }
            Ok(added)
        }
        Err(error) => {
            emit_created_section_diagnostic_for_error(&error);
            Err(errors::PendingWorkError::ApplicationWrite(
                error.to_string(),
            ))
        }
    }
}

fn build_add_command(
    cfg: &Config,
    args: &Args,
    date: &str,
) -> Result<AddPendingWorkItem, errors::PendingWorkError> {
    let tags = super::tags::from_flags(&args.tag)?;
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

    Ok(AddPendingWorkItem {
        project_name,
        prompt,
        title,
        created: date.to_string(),
        section: section.map(|section| section.as_str().to_string()),
        prereq,
        effort: args.effort,
        tags,
    })
}

#[cfg(test)]
mod tests {
    use std::assert_matches;

    use super::*;

    fn command_for(action: Action) -> (tempfile::TempDir, PendingWorkCommand) {
        let stage = tempfile::tempdir().unwrap();
        let config = stage.path().join("config.json");
        std::fs::write(
            &config,
            format!(
                r#"{{ "notesDir": "{}", "projects": {{ "pwf": "/repo" }}, "prefixes": {{ "pwf": "PWF" }} }}"#,
                stage.path().to_string_lossy().replace('\\', "\\\\")
            ),
        )
        .unwrap();
        let command = PendingWorkCommand::new(
            action,
            Args {
                config_path: Some(config.to_string_lossy().into_owned()),
                ..Args::default()
            },
        );
        (stage, command)
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
        let (_stage, command) = command_for(Action::Resolve);
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
