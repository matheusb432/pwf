//! `handoff new` — scaffold a handoff file and, for a managed repo, allocate its
//! linked pw work item.

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

pub(in crate::engines::handoff) fn invoke_new(
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
    let paths = handoff_paths(root);
    if !paths.dir.exists() {
        std::fs::create_dir_all(&paths.dir).map_err(|source| HandoffError::CreateDir {
            action: "new",
            path: paths.dir.clone(),
            source,
        })?;
    }
    let file_path = paths.dir.join(format!("{today}-{slug_val}.md"));
    if file_path.exists() {
        return Err(HandoffError::HandoffAlreadyExists { path: file_path });
    }
    let project = resolve_project_for_repo(root, args);
    let project_label = project
        .clone()
        .unwrap_or_else(|| leaf_name(root).to_string());

    // Write initial scaffold without pw
    write_text_atomic(&file_path, &scaffold(title, &project_label, &today, None)).map_err(
        |source| HandoffError::Write {
            action: "new",
            path: file_path.clone(),
            source,
        },
    )?;

    // If the repo is managed, allocate a pw work-item id: spawn the injected script
    // (tests inject pw-stub.sh) or, in production, call the pending-work engine in-process.
    let pw = if let Some(proj) = project.as_deref() {
        let id = if let Some(script) = &args.pending_work_script {
            let cfg = args
                .config_path
                .clone()
                .or_else(config::default_config_path)
                .unwrap_or_default();
            spawn_pw_add(script, &cfg, &today, proj)?
        } else {
            inprocess_pw_add(args, &today, proj)?
        };
        // Insert pw: <id> after the created: line
        let content = std::fs::read_to_string(&file_path).map_err(|source| HandoffError::Read {
            action: "new",
            path: file_path.clone(),
            source,
        })?;
        let new_content = CREATED_LINE_RE
            .replace(&content, format!("$1\npw: {id}").as_str())
            .into_owned();
        write_text_atomic(&file_path, &new_content).map_err(|source| HandoffError::Write {
            action: "new",
            path: file_path.clone(),
            source,
        })?;
        Some(id)
    } else {
        None
    };

    refresh_ledger_typed(root)?;

    let mut out = format!("Created handoff {}", file_path.display());
    if let Some(p) = pw {
        let _ = write!(out, "\n  pw: {p}");
    }
    out.push_str("\n  Now fill the Goals + Context, then run: handoff done <id> when complete.");
    Ok(out)
}

fn leaf_name(p: &Path) -> &str {
    p.file_name().and_then(|n| n.to_str()).unwrap_or("repo")
}
