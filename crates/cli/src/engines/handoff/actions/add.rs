//! `handoff add` — scaffold a handoff file and allocate its linked pw work item.
//! Handoffs require a managed repo (PWF-0117): an unmanaged repo errors before
//! any file is written, rather than scaffolding an unlinked handoff.

use std::{fmt::Write, path::Path, sync::LazyLock};

use regex::Regex;

use crate::{
    cli::Args,
    config,
    engines::handoff::{
        errors::HandoffError,
        ledger::refresh_ledger_typed,
        paths::{get_today, handoff_paths, resolve_project_for_repo, slug},
        pw_bridge::{inprocess_pw_add, spawn_pw_add},
        scaffold::scaffold,
    },
    fs_atomic::write_text_atomic,
};

static CREATED_LINE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^(created: .*)$").unwrap());

pub(in crate::engines::handoff) fn invoke_add(
    root: &Path,
    args: &Args,
) -> Result<String, HandoffError> {
    let title = args.title.as_deref().ok_or(HandoffError::MissingTitle)?;
    let today = get_today(args.date.as_deref());
    let slug_val = if let Some(s) = &args.slug {
        slug(s)
    } else {
        slug(title)
    };

    // Resolve the managed project *before* touching the filesystem: an
    // unmanaged repo must get no writes at all, not even an empty
    // `docs/handoffs/` directory.
    let project =
        resolve_project_for_repo(root, args).ok_or_else(|| HandoffError::UnmanagedRepo {
            root: root.display().to_string(),
        })?;

    let paths = handoff_paths(root);
    if !paths.dir.exists() {
        std::fs::create_dir_all(&paths.dir).map_err(|source| HandoffError::CreateDir {
            action: "add",
            path: paths.dir.clone(),
            source,
        })?;
    }
    let file_path = paths.dir.join(format!("{today}-{slug_val}.md"));
    if file_path.exists() {
        return Err(HandoffError::HandoffAlreadyExists { path: file_path });
    }

    // Write the scaffold before allocating the pw item: `--continue-handoff`
    // (below) builds its prompt by reading the newest handoff file on disk, so
    // the file must exist first.
    write_text_atomic(&file_path, &scaffold(title, &project, &today, None)).map_err(|source| {
        HandoffError::Write {
            action: "add",
            path: file_path.clone(),
            source,
        }
    })?;

    // Allocate the linked pw work item: spawn the injected script (tests inject
    // pw-stub.sh) or, in production, call the pending-work engine in-process.
    // On failure, best-effort delete the scaffold just written above — an
    // allocation failure must not strand a handoff file with no `pw:` link.
    let id = match if let Some(script) = &args.pending_work_script {
        let cfg = args
            .config_path
            .clone()
            .or_else(config::default_config_path)
            .unwrap_or_default();
        spawn_pw_add(script, &cfg, &today, &project)
    } else {
        inprocess_pw_add(args, &today, &project)
    } {
        Ok(id) => id,
        Err(error) => {
            let _ = std::fs::remove_file(&file_path);
            return Err(error);
        }
    };

    // Insert pw: <id> after the created: line.
    let content = std::fs::read_to_string(&file_path).map_err(|source| HandoffError::Read {
        action: "add",
        path: file_path.clone(),
        source,
    })?;
    let new_content = CREATED_LINE_RE
        .replace(&content, format!("$1\npw: {id}").as_str())
        .into_owned();
    write_text_atomic(&file_path, &new_content).map_err(|source| HandoffError::Write {
        action: "add",
        path: file_path.clone(),
        source,
    })?;

    refresh_ledger_typed(root)?;

    let mut out = format!("Created handoff {}", file_path.display());
    let _ = write!(out, "\n  pw: {id}");
    let _ = write!(
        out,
        "\n  Now fill the Goals + Context; close with: pwf done --id {id}"
    );
    Ok(out)
}
