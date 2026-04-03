// Shared input resolution for the `new` and `add` verbs: both validate the project
// + prompt, resolve the managed project + its repo, and derive a session title.
// `add` takes them positionally (its missing-input error points at the positional
// form); the hidden `new` verb still takes `--project`/`--prompt` flags.

use super::errors;
use super::model::Action;
use super::query::resolve_project_repo;
use super::text::{inferred_title, normalize_title};
use crate::cli::Args;
use crate::config::Config;

/// Resolved, validated inputs shared by the `new` and `add` verbs. The prompt is
/// borrowed from `args` (zero-copy); the rest are resolved owned strings, so the
/// struct is a thin grouping the compiler lays out with no extra indirection.
#[derive(Debug)]
pub(super) struct NewAddInputs<'a> {
    pub project_name: String,
    pub repo: String,
    pub session: String,
    pub prompt: &'a str,
}

/// Missing project/prompt error, phrased for the verb the user actually ran:
/// `add` takes positional args (point at the canonical form), `new` takes flags.
fn missing_input_err(action: &Action, field: &str) -> String {
    match action {
        Action::Add => errors::ADD_HINT.to_string(),
        _ => format!("--{field} is required for {}.", action.as_str()),
    }
}

impl<'a> NewAddInputs<'a> {
    /// Validate `--project`/`--prompt`, resolve the managed project + its repo path,
    /// and derive the session title (explicit `--title`, else inferred from the
    /// prompt). `action` only shapes the "required for <verb>" error text.
    pub(super) fn resolve(cfg: &Config, args: &'a Args, action: Action) -> Result<Self, String> {
        let project_raw = args
            .project
            .as_deref()
            .ok_or_else(|| missing_input_err(&action, "project"))?;
        let prompt = args
            .prompt
            .as_deref()
            .filter(|p| !p.trim().is_empty())
            .ok_or_else(|| missing_input_err(&action, "prompt"))?;
        let (project_name, repo) = resolve_project_repo(cfg, project_raw)?;
        let session = match args.title.as_deref() {
            Some(t) if !t.trim().is_empty() => normalize_title(t),
            _ => inferred_title(prompt),
        };
        Ok(Self {
            project_name,
            repo,
            session,
            prompt,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::errors;
    use super::*;

    fn cfg() -> Config {
        // "a" maps to a repo; "blank" is managed but has no repo mapping.
        let json = r#"{ "notesDir": "/n", "projects": { "alpha": "/repo/a", "blank": "" } }"#;
        crate::config::from_json(json, None).unwrap()
    }

    fn args(project: Option<&str>, prompt: Option<&str>, title: Option<&str>) -> Args {
        Args {
            project: project.map(str::to_string),
            prompt: prompt.map(str::to_string),
            title: title.map(str::to_string),
            ..Args::default()
        }
    }

    #[test]
    fn resolves_project_repo_prompt_and_inferred_session() {
        let cfg = cfg();
        let args = args(Some("alpha"), Some("fix the bug to satisfy CI"), None);
        let got = NewAddInputs::resolve(&cfg, &args, Action::Add).unwrap();
        assert_eq!(got.project_name, "alpha");
        assert_eq!(got.repo, "/repo/a");
        assert_eq!(got.prompt, "fix the bug to satisfy CI");
        assert_eq!(got.session, inferred_title("fix the bug to satisfy CI"));
    }

    #[test]
    fn explicit_title_overrides_inferred_session() {
        let cfg = cfg();
        let args = args(Some("alpha"), Some("do x"), Some("Custom Title"));
        let got = NewAddInputs::resolve(&cfg, &args, Action::New).unwrap();
        // Explicit titles are normalized to lowercase, same as inferred ones.
        assert_eq!(got.session, "custom title");
    }

    #[test]
    fn blank_title_falls_back_to_inferred_session() {
        let cfg = cfg();
        let args = args(Some("alpha"), Some("do x"), Some("   "));
        let got = NewAddInputs::resolve(&cfg, &args, Action::New).unwrap();
        assert_eq!(got.session, inferred_title("do x"));
    }

    #[test]
    fn unique_prefix_resolves_to_full_project_name() {
        let cfg = cfg();
        let args = args(Some("al"), Some("do x"), None);
        let got = NewAddInputs::resolve(&cfg, &args, Action::Add).unwrap();
        assert_eq!(got.project_name, "alpha");
    }

    #[test]
    fn missing_project_errors_with_verb_label() {
        let cfg = cfg();
        let args = args(None, Some("do x"), None);
        let err = NewAddInputs::resolve(&cfg, &args, Action::New).unwrap_err();
        assert_eq!(err, "--project is required for new.");
    }

    #[test]
    fn missing_or_blank_prompt_on_new_names_the_flag() {
        let cfg = cfg();
        for prompt in [None, Some("   ")] {
            let args = args(Some("alpha"), prompt, None);
            let err = NewAddInputs::resolve(&cfg, &args, Action::New).unwrap_err();
            assert_eq!(err, "--prompt is required for new.");
        }
    }

    #[test]
    fn missing_input_on_add_points_at_positional_form() {
        let cfg = cfg();
        // `add` takes positional args — the error must not name removed flags.
        let no_project = args(None, Some("do x"), None);
        assert_eq!(
            NewAddInputs::resolve(&cfg, &no_project, Action::Add).unwrap_err(),
            errors::ADD_HINT
        );
        let no_prompt = args(Some("alpha"), None, None);
        assert_eq!(
            NewAddInputs::resolve(&cfg, &no_prompt, Action::Add).unwrap_err(),
            errors::ADD_HINT
        );
    }

    #[test]
    fn managed_project_without_repo_mapping_errors() {
        let cfg = cfg();
        let args = args(Some("blank"), Some("do x"), None);
        let err = NewAddInputs::resolve(&cfg, &args, Action::Add).unwrap_err();
        assert_eq!(err, errors::not_mapped_to_repo("blank"));
    }
}
