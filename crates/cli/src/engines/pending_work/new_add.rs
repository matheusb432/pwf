use pwf_domain::pending_work::{inferred_title, normalize_title, title_was_normalized};

use super::{errors::PendingWorkError, query::resolve_project_repo};
use crate::{cli::EngineArgs, config::Config};

#[derive(Debug)]
pub(super) struct NewAddInputs<'a> {
    pub project_name: String,
    pub session: String,
    pub prompt: &'a str,
    /// True when the explicit `--title` needed YAML-safety rewriting beyond trim+lowercase.
    pub title_normalized: bool,
}

impl<'a> NewAddInputs<'a> {
    pub(super) fn resolve(cfg: &Config, args: &'a EngineArgs) -> Result<Self, PendingWorkError> {
        let project_raw = args.project.as_deref().ok_or(PendingWorkError::AddUsage)?;
        let prompt = args
            .prompt
            .as_deref()
            .filter(|p| !p.trim().is_empty())
            .ok_or(PendingWorkError::AddUsage)?;
        let (project_name, _repo) = resolve_project_repo(cfg, project_raw)?;
        let (session, title_normalized) = match args.title.as_deref() {
            Some(t) if !t.trim().is_empty() => (normalize_title(t), title_was_normalized(t)),
            _ => (inferred_title(prompt), false),
        };
        Ok(Self {
            project_name,
            session,
            prompt,
            title_normalized,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::assert_matches;

    use super::{super::errors, *};

    fn cfg() -> Config {
        let json = r#"{ "notesDir": "/n", "projects": { "alpha": "/repo/a", "blank": "" } }"#;
        crate::config::from_json(json, None).unwrap()
    }

    fn args(project: Option<&str>, prompt: Option<&str>, title: Option<&str>) -> EngineArgs {
        EngineArgs {
            project: project.map(str::to_string),
            prompt: prompt.map(str::to_string),
            title: title.map(str::to_string),
            ..EngineArgs::default()
        }
    }

    #[test]
    fn resolves_project_repo_prompt_and_inferred_session() {
        let cfg = cfg();
        let args = args(Some("alpha"), Some("fix the bug to satisfy CI"), None);
        let got = NewAddInputs::resolve(&cfg, &args).unwrap();
        assert_eq!(got.project_name, "alpha");
        assert_eq!(got.prompt, "fix the bug to satisfy CI");
        assert_eq!(got.session, inferred_title("fix the bug to satisfy CI"));
    }

    #[test]
    fn explicit_title_overrides_inferred_session() {
        let cfg = cfg();
        let args = args(Some("alpha"), Some("do x"), Some("Custom Title"));
        let got = NewAddInputs::resolve(&cfg, &args).unwrap();
        assert_eq!(got.session, "custom title");
        assert!(!got.title_normalized);
    }

    #[test]
    fn yaml_breaking_explicit_title_is_normalized_and_flagged() {
        let cfg = cfg();
        let args = args(Some("alpha"), Some("do x"), Some("fix bug: handle colons"));
        let got = NewAddInputs::resolve(&cfg, &args).unwrap();
        assert_eq!(got.session, "fix bug; handle colons");
        assert!(got.title_normalized);
    }

    #[test]
    fn yaml_breaking_inferred_title_normalizes_without_flagging() {
        let cfg = cfg();
        let args = args(Some("alpha"), Some("fix bug: empty prompt"), None);
        let got = NewAddInputs::resolve(&cfg, &args).unwrap();
        assert_eq!(got.session, "fix bug; empty prompt");
        assert!(!got.title_normalized);
    }

    #[test]
    fn blank_title_falls_back_to_inferred_session() {
        let cfg = cfg();
        let args = args(Some("alpha"), Some("do x"), Some("   "));
        let got = NewAddInputs::resolve(&cfg, &args).unwrap();
        assert_eq!(got.session, inferred_title("do x"));
    }

    #[test]
    fn marker_first_prompt_uses_domain_default_session() {
        let cfg = cfg();
        let args = args(Some("alpha"), Some("/c context"), None);

        let got = NewAddInputs::resolve(&cfg, &args).unwrap();

        assert_eq!(got.session, "n/a");
    }

    #[test]
    fn project_name_prefix_is_rejected_on_add() {
        let cfg = cfg();
        let args = args(Some("al"), Some("do x"), None);
        let err = NewAddInputs::resolve(&cfg, &args).unwrap_err();
        assert_matches!(err, errors::PendingWorkError::UnknownManagedProject { .. });
    }

    #[test]
    fn missing_input_on_add_points_at_positional_form() {
        let cfg = cfg();
        let no_project = args(None, Some("do x"), None);
        let err = NewAddInputs::resolve(&cfg, &no_project).unwrap_err();
        assert_matches!(err, errors::PendingWorkError::AddUsage);
        assert_eq!(err.to_string(), errors::ADD_HINT);

        let no_prompt = args(Some("alpha"), None, None);
        let err = NewAddInputs::resolve(&cfg, &no_prompt).unwrap_err();
        assert_matches!(err, errors::PendingWorkError::AddUsage);
        assert_eq!(err.to_string(), errors::ADD_HINT);
    }

    #[test]
    fn managed_project_without_repo_mapping_errors() {
        let cfg = cfg();
        let args = args(Some("blank"), Some("do x"), None);
        let err = NewAddInputs::resolve(&cfg, &args).unwrap_err();
        assert_eq!(
            err.to_string(),
            "Project 'blank' is not mapped to a repo in config/pending-work.json."
        );
    }
}
