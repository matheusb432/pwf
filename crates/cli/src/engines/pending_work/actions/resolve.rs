use std::path::Path;

use pwf_application::{
    AppDbStore, NoteMarkdownSource, PendingWorkItem,
    pending_work::{
        resolve::{ResolvePendingWorkError, ResolvePendingWorkItem},
        show::ShowPendingWorkItem,
    },
};

use super::super::errors::PendingWorkError;
use crate::{
    cli::Args,
    config::Config,
    engines::pending_work::{
        canonical_pending_id,
        run::{project_registry, require_id},
    },
};

/// Resolve the lookup id to feed the handlers, preferring the raw input when it
/// canonicalizes to the same id (so a not-found error preserves the raw form).
fn lookup_id<'a>(args: &'a Args, action: &'static str) -> Result<&'a str, PendingWorkError> {
    let canonical = require_id(args, action)?;
    Ok(match args.raw_id.as_deref() {
        Some(raw) if canonical_pending_id(raw) == canonical => raw,
        _ => canonical,
    })
}

/// Resolves an id to its note path (`show == false`) or full markdown source
/// (`show == true`). Every id shape goes through the application handlers —
/// the resolve handler serves legacy inline `project:N` prompts by scanning
/// the generic list for their ordinal.
pub(in crate::engines::pending_work) fn resolve_output<S>(
    cfg: &Config,
    store: &S,
    args: &Args,
    action: &'static str,
    show: bool,
) -> Result<String, PendingWorkError>
where
    S: AppDbStore<PendingWorkItem> + NoteMarkdownSource,
{
    let id = lookup_id(args, action)?;
    let registry = project_registry(cfg);
    if show {
        return match pwf_application::pending_work::show::execute(
            ShowPendingWorkItem { id: id.to_string() },
            store,
            &registry,
        ) {
            Ok(shown) => Ok(shown.markdown),
            // A missing-note wikilink has no markdown to stream. Attempt the
            // real read of the expected note so the surfaced failure is
            // literally the legacy one (`Cannot read item file: <io error>`),
            // not a reconstructed lookalike.
            Err(ResolvePendingWorkError::NoteFileMissing { path }) => {
                NoteMarkdownSource::read_note_markdown(store, Path::new(&path))
                    .map_err(|error| PendingWorkError::ApplicationRead(error.to_string()))
            }
            Err(error) => Err(PendingWorkError::ApplicationRead(error.to_string())),
        };
    }
    pwf_application::pending_work::resolve::execute(
        ResolvePendingWorkItem { id: id.to_string() },
        store,
        &registry,
    )
    .map(|resolved| resolved.note_path)
    .map_err(|error| PendingWorkError::ApplicationRead(error.to_string()))
}

pub(in crate::engines::pending_work) fn run_resolve<S>(
    cfg: &Config,
    store: &S,
    args: &Args,
) -> Result<String, PendingWorkError>
where
    S: AppDbStore<PendingWorkItem> + NoteMarkdownSource,
{
    resolve_output(cfg, store, args, "resolve", args.show)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::pending_work::run::store_for;

    /// Stage a vault whose index links PWF-0001 with no backing note file.
    fn ghost_stage() -> (tempfile::TempDir, Config) {
        let stage = tempfile::tempdir().unwrap();
        let notes = stage.path().join("notes");
        let proj = notes.join("pwf");
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::write(
            proj.join("pwf.md"),
            "---\nid: pwf\ntitle: pwf\n---\n\n- [ ] [[PWF-0001|ghost]]\n",
        )
        .unwrap();
        let cfg = crate::config::from_json(
            &format!(
                r#"{{ "notesDir": {}, "projects": {{ "pwf": "/repo/pwf" }}, "prefixes": {{ "pwf": "PWF" }} }}"#,
                serde_json::to_string(&notes.to_string_lossy()).unwrap()
            ),
            None,
        )
        .unwrap();
        (stage, cfg)
    }

    fn id_args(id: &str) -> Args {
        Args {
            id: Some(id.to_string()),
            ..Args::default()
        }
    }

    #[test]
    fn show_of_missing_note_wikilink_errors_like_the_legacy_read() {
        let (_stage, cfg) = ghost_stage();
        let store = store_for(&cfg);

        let error = resolve_output(&cfg, &store, &id_args("PWF-0001"), "show", true).unwrap_err();

        // The legacy read failure, literally: resolve_note_file → read_item_file.
        assert!(
            error.to_string().starts_with("Cannot read item file: "),
            "got: {error}"
        );
    }

    #[test]
    fn resolve_of_missing_note_wikilink_still_prints_expected_path() {
        let (_stage, cfg) = ghost_stage();
        let store = store_for(&cfg);

        let path = resolve_output(&cfg, &store, &id_args("PWF-0001"), "resolve", false).unwrap();

        assert!(path.ends_with("/pwf/PWF-0001.md"), "got: {path}");
    }
}
