use std::path::{Path, PathBuf};

use super::errors::PendingWorkError;

pub(super) fn newest_handoff(repo: &str) -> Result<PathBuf, PendingWorkError> {
    let handoff_dir = Path::new(repo).join("docs").join("handoffs");
    if !handoff_dir.exists() {
        return Err(PendingWorkError::NoHandoffDirectory { path: handoff_dir });
    }
    let excluded = ["LEDGER.md", "README.md"];
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(&handoff_dir)
        .map_err(|source| PendingWorkError::ReadHandoffDirectory {
            path: handoff_dir.clone(),
            source,
        })?
        .flatten()
    {
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("md") {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("")
            .to_string();
        if excluded
            .iter()
            .any(|excluded| excluded.eq_ignore_ascii_case(&name))
        {
            continue;
        }
        let modified = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .unwrap_or(std::time::UNIX_EPOCH);
        entries.push((modified, name, path));
    }
    if entries.is_empty() {
        return Err(PendingWorkError::NoHandoffMarkdown { path: handoff_dir });
    }
    entries.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| right.1.cmp(&left.1)));
    Ok(entries
        .into_iter()
        .next()
        .expect("non-empty handoff entries")
        .2)
}
