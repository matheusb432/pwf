use pwf_application::{
    AppDbStore, IndexEntry, IndexSection, NoteMarkdownSource, PendingWorkItem,
    pending_work::{
        add::AddPendingWorkItem,
        session::{
            Agent, ConfirmationPolicy, DispatchMode, LaunchDirectives, dispatch::DispatchSession,
            verify::VerifySession,
        },
    },
};
use pwf_domain::pending_work::{HANDOFF_TAG, MutationOutcome, ProjectName, ProjectRegistry};
use pwf_infra::{
    obsidian::ObsidianStore,
    session::{ProcessSessionRuntime, TomlModelTierCatalog},
};

use super::{
    actions::{
        AddedItem, EngineOutcome, ListParams, TITLE_NORMALIZED_NOTICE,
        emit_created_section_diagnostic, emit_created_section_diagnostic_for_error,
        list::{ListScope, OrderSpec},
        render_outcome_confirmation, run_cancel, run_done, run_list_query, run_remove, run_reopen,
        run_update,
    },
    agent::verify,
    color::use_color,
    continue_prompt::{continue_handoff_prompt, continue_plan_prompt},
    domain::commands::PendingWorkCommand,
    errors,
    model::Action,
    naming::stamp_date,
    new_add::NewAddInputs,
    query::{load_config, resolve_managed_project_name_typed, resolve_project_repo},
    route::run_route,
    section::Section,
};
use crate::{
    cli::EngineArgs,
    config::Config,
    confirm::{Confirmation, DefaultAnswer},
    engines::{handoff::mirror, pending_work::actions::run_show},
};

/// Runs a parsed command and applies terminal formatting to typed mutations.
/// `run_args` retains the legacy raw-text protocol for non-terminal seams.
pub fn run(command: &PendingWorkCommand) -> Result<String, String> {
    let outcome = run_typed(command, &crate::confirm::terminal).map_err(String::from)?;
    let on = use_color(command.args().color);
    match render_outcome_confirmation(&outcome, on) {
        Some(rendered) => Ok(rendered),
        None => Ok(outcome.into_raw_text()),
    }
}

/// Constructs [`ObsidianStore`] at the CLI composition root.
/// Downstream code receives only the store traits it requires.
pub(crate) fn store_for(
    cfg: &Config,
) -> impl NoteMarkdownSource
+ AppDbStore<PendingWorkItem>
+ AppDbStore<IndexEntry>
+ AppDbStore<IndexSection>
+ use<> {
    ObsidianStore::new(cfg.clone())
}

/// Builds the [`ProjectRegistry`] and canonicalizes ID prefixes to uppercase.
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
    confirmation: &impl Fn(&str, DefaultAnswer) -> Confirmation,
) -> Result<EngineOutcome, errors::PendingWorkError> {
    let args = command.args();

    let cfg = load_config(args)?;
    let store = store_for(&cfg);
    let projects = project_registry(&cfg);
    let model_tiers = TomlModelTierCatalog;
    let runtime = ProcessSessionRuntime;
    let interaction = super::session::CliSessionInteraction;
    let date = stamp_date(args.date.as_deref());

    match command.action() {
        Action::Route => Ok(EngineOutcome::Text(run_route(
            &cfg,
            &store,
            &projects,
            &model_tiers,
            &runtime,
            args,
            confirmation,
        )?)),

        Action::Add => Ok(EngineOutcome::Mutation(MutationOutcome::Added(run_add(
            &cfg, &store, args, &date,
        )?))),

        Action::List => run_list(&cfg, &store, args),

        Action::Verify => {
            let request = VerifySession {
                id: args.id.clone(),
                agent: application_agent(args.agent),
                model_override: args.model.clone(),
            };
            let outcome = pwf_application::pending_work::session::verify::execute(
                request,
                &store,
                &projects,
                &model_tiers,
                &runtime,
            )?;
            Ok(EngineOutcome::Text(verify::render(&outcome)))
        }

        Action::Done => Ok(EngineOutcome::Text(run_done(&cfg, &store, args)?)),

        Action::Cancel => Ok(EngineOutcome::Text(run_cancel(&cfg, &store, args)?)),

        Action::Reopen => Ok(EngineOutcome::Text(run_reopen(&cfg, &store, args)?)),

        Action::Show => Ok(EngineOutcome::Text(run_show(&cfg, &store, args)?)),

        Action::Remove => run_remove(&cfg, &store, args, confirmation),

        Action::Update => Ok(EngineOutcome::Mutation(MutationOutcome::Updated(
            run_update(&store, &projects, args)?,
        ))),

        Action::Session => {
            let id = require_id(args, "session")?;
            // Apply append before dispatch so the launch prompt includes the new body text.
            if args.append.is_some() {
                run_update(&store, &projects, args)?;
            }
            let request = DispatchSession {
                id: id.to_string(),
                mode: if args.inline {
                    DispatchMode::Inline
                } else {
                    DispatchMode::Multiplexer
                },
                directives: LaunchDirectives {
                    worktree: args.worktree,
                    autonomous: args.auto,
                },
                agent: application_agent(args.agent),
                model_override: args.model.clone(),
                confirmation: if args.assume_yes {
                    ConfirmationPolicy::Skip
                } else {
                    ConfirmationPolicy::Ask
                },
            };
            let outcome = pwf_application::pending_work::session::dispatch::execute(
                &request,
                &store,
                &projects,
                &model_tiers,
                &runtime,
                &interaction,
            )?;
            Ok(EngineOutcome::Text(super::session::render_dispatch(
                &outcome,
                use_color(args.color),
            )))
        }
    }
}

fn application_agent(agent: crate::cli::Agent) -> Agent {
    match agent {
        crate::cli::Agent::Claude => Agent::Claude,
        crate::cli::Agent::Codex => Agent::Codex,
    }
}

fn run_list(
    cfg: &Config,
    store: &impl AppDbStore<PendingWorkItem>,
    args: &EngineArgs,
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
        status_filter: args.status_filter,
        color_on: use_color(args.color),
    };
    Ok(EngineOutcome::Text(run_list_query(cfg, store, params)?))
}

pub fn run_args(args: &crate::cli::EngineArgs) -> Result<String, String> {
    run_args_typed(args).map_err(String::from)
}

pub(in crate::engines::pending_work) fn run_args_typed(
    args: &crate::cli::EngineArgs,
) -> Result<String, errors::PendingWorkError> {
    let command = PendingWorkCommand::from_args_typed(args)?;
    run_typed(&command, &|_, _| Confirmation::NonInteractive).map(EngineOutcome::into_raw_text)
}

/// Resolves `--section`, which takes precedence over `--human`.
fn resolve_add_section(args: &EngineArgs) -> Result<Option<Section>, errors::PendingWorkError> {
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
    args: &'args EngineArgs,
    action: &'static str,
) -> Result<&'args str, errors::PendingWorkError> {
    args.id
        .as_deref()
        .ok_or(errors::PendingWorkError::MissingId { action })
}

/// Builds the add request, store, and registry for CLI and in-process handoff creation.
/// Constructing the concrete store here keeps the handoff bridge independent of [`ObsidianStore`].
pub(crate) fn add_command_from_args(
    args: &EngineArgs,
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
    let (command, _title_normalized) = build_add_command(&cfg, args, &date)?;
    Ok((store, registry, command))
}

/// Creates an item after preflighting any new handoff scaffold.
/// `--continue-handoff` targets an existing handoff and never creates a scaffold.
fn run_add<S>(
    cfg: &Config,
    store: &S,
    args: &EngineArgs,
    date: &str,
) -> Result<AddedItem, errors::PendingWorkError>
where
    S: AppDbStore<PendingWorkItem> + AppDbStore<IndexEntry> + AppDbStore<IndexSection>,
{
    let (command, title_normalized) = build_add_command(cfg, args, date)?;

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
            if title_normalized {
                eprintln!("{TITLE_NORMALIZED_NOTICE}");
            }
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

/// Builds the add request plus whether an explicit `--title` needed YAML-safety rewriting.
fn build_add_command(
    cfg: &Config,
    args: &EngineArgs,
    date: &str,
) -> Result<(AddPendingWorkItem, bool), errors::PendingWorkError> {
    let tags = super::tags::from_flags(&args.tag)?;
    let section = resolve_add_section(args)?;
    let prereq = super::prereq::frontmatter_from_flags(cfg, &args.prereq)?;

    let (project_name, title, prompt, title_normalized): (String, Option<String>, String, bool) =
        if args.continue_handoff {
            let raw = args
                .project
                .as_deref()
                .ok_or(errors::PendingWorkError::AddUsage)?;
            let (project_name, repo) = resolve_project_repo(cfg, raw)?;
            let (title, prompt) = continue_handoff_prompt(&repo)?;
            (project_name, Some(title), prompt, false)
        } else if let Some(path) = args.continue_path.as_deref() {
            let raw = args
                .project
                .as_deref()
                .ok_or(errors::PendingWorkError::AddUsage)?;
            let (project_name, _repo) = resolve_project_repo(cfg, raw)?;
            let (title, prompt) = continue_plan_prompt(&project_name, path);
            (project_name, Some(title), prompt, false)
        } else {
            let input = NewAddInputs::resolve(cfg, args)?;
            (
                input.project_name,
                Some(input.session),
                input.prompt.to_string(),
                input.title_normalized,
            )
        };

    Ok((
        AddPendingWorkItem {
            project_name,
            prompt,
            title,
            created: date.to_string(),
            section: section.map(|section| section.as_str().to_string()),
            prereq,
            effort: args.effort,
            tags,
        },
        title_normalized,
    ))
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
            EngineArgs {
                config_path: Some(config.to_string_lossy().into_owned()),
                ..EngineArgs::default()
            },
        );
        (stage, command)
    }

    #[test]
    fn run_args_stringifies_missing_action_at_public_edge() {
        assert_eq!(
            run_args(&EngineArgs::default()).unwrap_err(),
            "a pw subcommand is required."
        );
    }

    #[test]
    fn run_args_stringifies_unknown_action_at_public_edge() {
        let args = EngineArgs {
            action: Some("nope".to_string()),
            ..EngineArgs::default()
        };

        assert_eq!(run_args(&args).unwrap_err(), "Unknown action: nope");
    }

    #[test]
    fn show_missing_id_returns_typed_error_with_legacy_display() {
        let (_stage, command) = command_for(Action::Show);
        let err = require_id(command.args(), "show").unwrap_err();

        assert_matches!(
            err,
            errors::PendingWorkError::MissingId { action } if action == "show"
        );
        assert_eq!(err.to_string(), "--id is required for show.");
    }

    #[test]
    fn section_flag_wins_over_human_shorthand() {
        let args = EngineArgs {
            section: Some("future".into()),
            human: true,
            ..EngineArgs::default()
        };
        assert_eq!(resolve_add_section(&args).unwrap(), Some(Section::Future));
    }

    #[test]
    fn human_shorthand_maps_to_human_section() {
        let args = EngineArgs {
            human: true,
            ..EngineArgs::default()
        };
        assert_eq!(resolve_add_section(&args).unwrap(), Some(Section::Human));
    }

    #[test]
    fn no_section_flags_means_general_area() {
        assert_eq!(resolve_add_section(&EngineArgs::default()).unwrap(), None);
    }

    #[test]
    fn invalid_section_value_errors() {
        let args = EngineArgs {
            section: Some("bogus".into()),
            ..EngineArgs::default()
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
