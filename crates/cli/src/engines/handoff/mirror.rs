//! Tag-governed mirroring (PWF-0117): pending-work verbs on a `handoff`-tagged
//! item perform the matching operation on the item's handoff file. Gate +
//! forward resolution live here; the close/reopen/delete/scaffold ops below
//! are the actual mirroring work, ported from `actions/complete.rs` and
//! `actions/reopen.rs`'s transform + rollback logic rather than reinvented.

use std::{
    path::{Path, PathBuf},
    sync::LazyLock,
};

use pwf_domain::pending_work::{HANDOFF_TAG, Tags};
use pwf_infra::obsidian::ObsidianPendingWorkStore;
use regex::Regex;

use super::{
    errors::HandoffError,
    ledger::{HandoffEntry, get_active_handoff_files, read_handoff_entries, refresh_ledger_typed},
    paths::{expand_home, handoff_paths, slug},
    scaffold::scaffold,
};
use crate::{config::Config, frontmatter, fs_atomic::write_text_atomic, regexes::STATUS_LINE_RE};

/// Errors from the handoff mirror gate and (in a later task) its mirroring ops.
#[derive(Debug, thiserror::Error)]
pub(crate) enum MirrorError {
    /// The item's `tags:` frontmatter value could not be parsed.
    #[error("item {id} has invalid tags frontmatter: {raw}")]
    InvalidTags {
        /// The gated item's canonical id.
        id: String,
        /// The raw, unparseable `tags:` value.
        raw: String,
    },
    /// A `handoff`-tagged item's project has no repo mapping in config.
    #[error("item {id} is tagged `handoff` but project {project} maps to no repo in the config")]
    UnmanagedProject {
        /// The gated item's canonical id.
        id: String,
        /// The item's managed project name.
        project: String,
    },
    /// A `handoff`-tagged item's mapped repo path does not exist on disk.
    #[error("item {id} is tagged `handoff` but its repo path does not exist: {}", path.display())]
    RepoRootMissing {
        /// The gated item's canonical id.
        id: String,
        /// The nonexistent, already-expanded repo path.
        path: PathBuf,
    },
    /// No handoff file in the repo's handoff dir links back to this item.
    #[error(
        "item {id} is tagged `handoff` but no handoff with `pw: {id}` exists in {} — untag it (`pwf update --id {id} --tags-clear`) or create the handoff",
        dir.display()
    )]
    HandoffNotFound {
        /// The gated item's canonical id.
        id: String,
        /// The handoff dir that was searched.
        dir: PathBuf,
    },
    /// More than one handoff file claims the same linked item.
    #[error("more than one handoff in {} claims `pw: {id}` — fix the duplicate frontmatter", dir.display())]
    AmbiguousHandoff {
        /// The gated item's canonical id.
        id: String,
        /// The handoff dir that was searched.
        dir: PathBuf,
    },
    /// The archive-move target already exists.
    #[error("archived handoff already exists: {}", path.display())]
    ArchiveAlreadyExists {
        /// The already-occupied archive path.
        path: PathBuf,
    },
    /// The un-archive-move target already exists.
    #[error("active handoff already exists: {}", path.display())]
    HandoffAlreadyExists {
        /// The already-occupied active path.
        path: PathBuf,
    },
    /// A filesystem operation on a handoff file failed.
    #[error("{source}")]
    Io {
        /// The attempted operation, for context in logs/tests.
        action: &'static str,
        /// The path the operation targeted.
        path: PathBuf,
        /// The underlying I/O error.
        source: std::io::Error,
    },
    /// Rebuilding the LEDGER after a mirror op failed.
    #[error("{message}")]
    Ledger {
        /// The rendered ledger-refresh failure.
        message: String,
    },
}

impl From<HandoffError> for MirrorError {
    /// A ledger-refresh failure inside a mirror op is always reported through
    /// `Ledger` — callers don't need to know it started life as a `HandoffError`.
    fn from(e: HandoffError) -> Self {
        MirrorError::Ledger {
            message: e.to_string(),
        }
    }
}

/// The `handoff`-tagged item a mutation gate resolved, and where its repo lives.
#[derive(Debug)]
pub(crate) struct GateItem {
    /// The item's canonical id.
    pub id: String,
    /// The item's repo root, already `~`-expanded and confirmed to exist.
    pub repo_root: PathBuf,
}

/// `Ok(Some)` iff `id` resolves to a note whose tags contain `handoff`.
/// Missing note / no tags / untagged → `Ok(None)` so the pw handler keeps
/// producing its canonical not-found/skip behavior.
pub(crate) fn handoff_gate(cfg: &Config, id: &str) -> Result<Option<GateItem>, MirrorError> {
    let store = ObsidianPendingWorkStore::new(cfg.clone());
    let Some((project, note_path)) = store.note_with_project(id) else {
        return Ok(None);
    };
    let Ok(raw) = std::fs::read_to_string(&note_path) else {
        return Ok(None);
    };
    let parsed = frontmatter::parse(&raw);
    let Some(tags_raw) = parsed.frontmatter.get("tags") else {
        return Ok(None);
    };
    let canonical_id = note_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(id)
        .to_string();
    let tags = Tags::parse_frontmatter(tags_raw).map_err(|_| MirrorError::InvalidTags {
        id: canonical_id.clone(),
        raw: tags_raw.clone(),
    })?;
    if !tags.contains_name(HANDOFF_TAG) {
        return Ok(None);
    }
    let repo_raw = cfg.projects.get(&project).cloned().unwrap_or_default();
    if repo_raw.trim().is_empty() {
        return Err(MirrorError::UnmanagedProject {
            id: canonical_id,
            project,
        });
    }
    let repo_root = PathBuf::from(expand_home(&repo_raw));
    if !repo_root.exists() {
        return Err(MirrorError::RepoRootMissing {
            id: canonical_id,
            path: repo_root,
        });
    }
    Ok(Some(GateItem {
        id: canonical_id,
        repo_root,
    }))
}

/// Capturing `status:` variant (group `$1`); distinct from the shared,
/// non-capturing `crate::regexes::STATUS_LINE_RE`. Ported from
/// `actions/complete.rs`.
static STATUS_CAPTURE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^(status:.*)$").unwrap());

/// Insert/replace a field after `status:`. Ported from `actions/complete.rs`,
/// which now imports it here instead of holding its own copy.
pub(super) fn set_frontmatter_field(content: &str, field: &str, value: &str) -> String {
    let field_re = Regex::new(&format!(r"(?m)^{field}:.*$")).unwrap();
    if field_re.is_match(content) {
        field_re
            .replace(content, format!("{field}: {value}").as_str())
            .into_owned()
    } else {
        // Insert after the first status: line
        STATUS_CAPTURE_RE
            .replace(content, format!("$1\n{field}: {value}").as_str())
            .into_owned()
    }
}

/// Drop a frontmatter field line (and its trailing newline) entirely. Ported
/// from `actions/reopen.rs`, which now imports it here instead of holding its
/// own copy. Used by `preflight_reopen` to drop the `completed:` stamp.
pub(super) fn drop_frontmatter_field(content: &str, field: &str) -> String {
    let re = Regex::new(&format!(r"(?m)^{field}:.*\n?")).unwrap();
    re.replace(content, "").into_owned()
}

/// Which terminal status a mirror close writes to the handoff file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MirrorClose {
    /// The linked pw item was completed normally.
    Done,
    /// The linked pw item was cancelled.
    Cancelled,
}

impl MirrorClose {
    fn status(self) -> &'static str {
        match self {
            MirrorClose::Done => "done",
            MirrorClose::Cancelled => "cancelled",
        }
    }
}

/// Find the single entry among `entries` whose `pw:` frontmatter matches `id`
/// case-insensitively. `dir` is only used to render the not-found/ambiguous
/// error's location.
fn find_by_pw<'a>(
    entries: &'a [HandoffEntry],
    id: &str,
    dir: &Path,
) -> Result<&'a HandoffEntry, MirrorError> {
    let mut matches = entries.iter().filter(|e| {
        e.frontmatter
            .get("pw")
            .is_some_and(|pw| pw.eq_ignore_ascii_case(id))
    });
    let first = matches.next().ok_or_else(|| MirrorError::HandoffNotFound {
        id: id.to_string(),
        dir: dir.to_path_buf(),
    })?;
    if matches.next().is_some() {
        return Err(MirrorError::AmbiguousHandoff {
            id: id.to_string(),
            dir: dir.to_path_buf(),
        });
    }
    Ok(first)
}

/// A staged, not-yet-applied archive/un-archive move: the destination content
/// is fully computed and all preconditions checked before any write happens.
/// `commit` performs the actual filesystem mutation with the rollback shape
/// ported from `actions/complete.rs`/`actions/reopen.rs`: write the
/// destination, remove the source, rebuild the ledger — unwinding on the
/// first failure so a partial move never strands both copies.
#[derive(Debug)]
pub(crate) struct PendingMove {
    root: PathBuf,
    src: PathBuf,
    src_original: String,
    dest: PathBuf,
    content: String,
}

impl PendingMove {
    pub(crate) fn commit(self) -> Result<PathBuf, MirrorError> {
        let PendingMove {
            root,
            src,
            src_original,
            dest,
            content,
        } = self;
        write_text_atomic(&dest, &content).map_err(|source| MirrorError::Io {
            action: "write-dest",
            path: dest.clone(),
            source,
        })?;
        if let Err(source) = std::fs::remove_file(&src) {
            let _ = std::fs::remove_file(&dest);
            return Err(MirrorError::Io {
                action: "remove-src",
                path: src,
                source,
            });
        }
        if let Err(e) = refresh_ledger_typed(&root) {
            let _ = write_text_atomic(&src, &src_original);
            let _ = std::fs::remove_file(&dest);
            return Err(MirrorError::from(e));
        }
        Ok(dest)
    }
}

/// Preflight a handoff close (`done`/`cancel`): locate the active handoff
/// linked to `gate.id`, compute its archived content, and check the archive
/// destination is free — without writing anything. `Ok(None)` means the
/// linked handoff is already archived: the pw item is being closed again
/// (e.g. a re-run of `done` after it already succeeded), so the pair is
/// already in its goal state and there is nothing left to mirror — the pw
/// handler's own already-closed error is what should surface, not a mirror
/// not-found. Transform ported from `actions/complete.rs::complete_handoff`.
pub(crate) fn preflight_close(
    gate: &GateItem,
    close: MirrorClose,
    date: &str,
    report: Option<&str>,
) -> Result<Option<PendingMove>, MirrorError> {
    let paths = handoff_paths(&gate.repo_root);
    let active = get_active_handoff_files(&paths.dir);
    let entry = match find_by_pw(&active, &gate.id, &paths.dir) {
        Ok(entry) => entry,
        Err(err @ MirrorError::HandoffNotFound { .. }) => {
            let archived = read_handoff_entries(&paths.archive);
            return match find_by_pw(&archived, &gate.id, &paths.archive) {
                // Already archived: nothing left to mirror.
                Ok(_) => Ok(None),
                // In neither dir: keep the active-dir not-found error, whose
                // rendered path points at where a close expects the handoff.
                Err(MirrorError::HandoffNotFound { .. }) => Err(err),
                Err(other) => Err(other),
            };
        }
        Err(err) => return Err(err),
    };
    let original = std::fs::read_to_string(&entry.full_path).map_err(|source| MirrorError::Io {
        action: "read-active",
        path: entry.full_path.clone(),
        source,
    })?;

    let content = STATUS_LINE_RE
        .replace(&original, format!("status: {}", close.status()).as_str())
        .into_owned();
    let content = set_frontmatter_field(&content, "completed", date);
    let content = if close == MirrorClose::Cancelled
        && let Some(reason) = report
    {
        format!("{}\n\n> Cancelled: {reason}\n", content.trim_end())
    } else {
        content
    };

    let dest = paths.archive.join(&entry.name);
    if dest.exists() {
        return Err(MirrorError::ArchiveAlreadyExists { path: dest });
    }

    Ok(Some(PendingMove {
        root: gate.repo_root.clone(),
        src: entry.full_path.clone(),
        src_original: original,
        dest,
        content,
    }))
}

/// Preflight a handoff reopen: locate the archived (non-active) handoff
/// linked to `gate.id`, compute its reactivated content, and check the active
/// destination is free — without writing anything. `Ok(None)` means the
/// linked handoff is already in the active dir: the pair is in its goal
/// state, so an idempotent reopen skip (FR-0021) has nothing to mirror.
/// Transform ported from `actions/reopen.rs::reopen_handoff`.
pub(crate) fn preflight_reopen(gate: &GateItem) -> Result<Option<PendingMove>, MirrorError> {
    let paths = handoff_paths(&gate.repo_root);
    let archived: Vec<HandoffEntry> = read_handoff_entries(&paths.archive)
        .into_iter()
        .filter(|e| e.frontmatter.get("status").map(String::as_str) != Some("active"))
        .collect();
    let entry = match find_by_pw(&archived, &gate.id, &paths.archive) {
        Ok(entry) => entry,
        Err(err @ MirrorError::HandoffNotFound { .. }) => {
            let active = get_active_handoff_files(&paths.dir);
            return match find_by_pw(&active, &gate.id, &paths.dir) {
                // Already active: nothing to un-archive.
                Ok(_) => Ok(None),
                // In neither dir: keep the archive-dir not-found error, whose
                // rendered path points at where a reopen expects the handoff.
                Err(MirrorError::HandoffNotFound { .. }) => Err(err),
                Err(other) => Err(other),
            };
        }
        Err(err) => return Err(err),
    };
    let original = std::fs::read_to_string(&entry.full_path).map_err(|source| MirrorError::Io {
        action: "read-archived",
        path: entry.full_path.clone(),
        source,
    })?;

    let content = STATUS_LINE_RE
        .replace(&original, "status: active")
        .into_owned();
    let content = drop_frontmatter_field(&content, "completed");

    let dest = paths.dir.join(&entry.name);
    if dest.exists() {
        return Err(MirrorError::HandoffAlreadyExists { path: dest });
    }

    Ok(Some(PendingMove {
        root: gate.repo_root.clone(),
        src: entry.full_path.clone(),
        src_original: original,
        dest,
        content,
    }))
}

/// Locate the active handoff linked to `gate.id` without deleting anything.
pub(crate) fn preflight_delete(gate: &GateItem) -> Result<PathBuf, MirrorError> {
    let paths = handoff_paths(&gate.repo_root);
    let active = get_active_handoff_files(&paths.dir);
    let entry = find_by_pw(&active, &gate.id, &paths.dir)?;
    Ok(entry.full_path.clone())
}

/// Delete the active handoff linked to `gate.id` and rebuild the ledger,
/// restoring the file if the rebuild fails so a delete can never silently
/// desync the ledger from what's actually on disk.
pub(crate) fn delete_for_item(gate: &GateItem) -> Result<PathBuf, MirrorError> {
    let path = preflight_delete(gate)?;
    let original = std::fs::read_to_string(&path).map_err(|source| MirrorError::Io {
        action: "read-active",
        path: path.clone(),
        source,
    })?;
    std::fs::remove_file(&path).map_err(|source| MirrorError::Io {
        action: "delete",
        path: path.clone(),
        source,
    })?;
    if let Err(e) = refresh_ledger_typed(&gate.repo_root) {
        let _ = write_text_atomic(&path, &original);
        return Err(MirrorError::from(e));
    }
    Ok(path)
}

/// A staged, not-yet-written new handoff file: the destination path is free
/// and the repo root resolved, but nothing is on disk until `commit`.
#[derive(Debug)]
pub(crate) struct PendingScaffold {
    path: PathBuf,
    project_label: String,
    title: String,
    date: String,
    root: PathBuf,
}

impl PendingScaffold {
    pub(crate) fn commit(self, pw_id: &str) -> Result<PathBuf, MirrorError> {
        let content = scaffold(&self.title, &self.project_label, &self.date, Some(pw_id));
        write_text_atomic(&self.path, &content).map_err(|source| MirrorError::Io {
            action: "write-scaffold",
            path: self.path.clone(),
            source,
        })?;
        if let Err(e) = refresh_ledger_typed(&self.root) {
            // Same never-strand-partial-state invariant as PendingMove::commit:
            // a failed ledger rebuild must not leave the new file on disk.
            let _ = std::fs::remove_file(&self.path);
            return Err(MirrorError::from(e));
        }
        Ok(self.path)
    }
}

/// Preflight a new handoff scaffold for `project`: resolve its repo root from
/// config and check the dated slug path is free — without writing anything.
/// `project` doubles as the id in `UnmanagedProject`/`RepoRootMissing` (no pw
/// item exists yet at this point — scaffolding happens before allocation),
/// mirroring `handoff_gate`'s own project-to-repo resolution.
pub(crate) fn preflight_scaffold(
    cfg: &Config,
    project: &str,
    title: &str,
    date: &str,
) -> Result<PendingScaffold, MirrorError> {
    let repo_raw = cfg.projects.get(project).cloned().unwrap_or_default();
    if repo_raw.trim().is_empty() {
        return Err(MirrorError::UnmanagedProject {
            id: project.to_string(),
            project: project.to_string(),
        });
    }
    let root = PathBuf::from(expand_home(&repo_raw));
    if !root.exists() {
        return Err(MirrorError::RepoRootMissing {
            id: project.to_string(),
            path: root,
        });
    }

    let paths = handoff_paths(&root);
    let path = paths.dir.join(format!("{date}-{}.md", slug(title)));
    if path.exists() {
        return Err(MirrorError::HandoffAlreadyExists { path });
    }

    Ok(PendingScaffold {
        path,
        project_label: project.to_string(),
        title: title.to_string(),
        date: date.to_string(),
        root,
    })
}

#[cfg(test)]
mod tests {
    use std::{assert_matches, fmt::Write as _, path::Path};

    use super::*;
    use crate::engines::handoff::test_support::tempdir;

    /// Write a minimal pending-work item note with optional `tags:` frontmatter.
    fn write_item_note(path: &Path, project: &str, tags: Option<&str>) {
        let mut note = format!(
            "---\nstatus: active\ntitle: test item\nproject: {project}\ncreated: 2026-07-01\n"
        );
        if let Some(tags) = tags {
            let _ = writeln!(note, "tags: {tags}");
        }
        note.push_str("---\n\nbody\n");
        std::fs::write(path, note).unwrap();
    }

    fn write_config(path: &Path, notes: &Path, project: &str, repo_raw: &str) {
        std::fs::write(
            path,
            format!(
                r#"{{ "notesDir": "{}", "projects": {{ "{project}": "{}" }}, "prefixes": {{ "{project}": "TST" }} }}"#,
                notes.to_string_lossy().replace('\\', "/"),
                repo_raw.replace('\\', "/"),
            ),
        )
        .unwrap();
    }

    #[test]
    fn untagged_item_gates_to_none() {
        let stage = tempdir();
        let notes = stage.path().join("notes");
        let project_dir = notes.join("test-project");
        std::fs::create_dir_all(&project_dir).unwrap();
        write_item_note(&project_dir.join("TST-0001.md"), "test-project", None);

        let repo = stage.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let cfg_path = stage.path().join("config.json");
        write_config(&cfg_path, &notes, "test-project", &repo.to_string_lossy());
        let cfg = crate::config::load(&cfg_path.to_string_lossy(), None).unwrap();

        let result = handoff_gate(&cfg, "TST-0001").unwrap();

        assert!(result.is_none());
    }

    #[test]
    fn missing_item_gates_to_none() {
        let stage = tempdir();
        let notes = stage.path().join("notes");
        let project_dir = notes.join("test-project");
        std::fs::create_dir_all(&project_dir).unwrap();

        let repo = stage.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let cfg_path = stage.path().join("config.json");
        write_config(&cfg_path, &notes, "test-project", &repo.to_string_lossy());
        let cfg = crate::config::load(&cfg_path.to_string_lossy(), None).unwrap();

        let result = handoff_gate(&cfg, "TST-9999").unwrap();

        assert!(result.is_none());
    }

    #[test]
    fn tagged_item_with_existing_repo_gates_to_some_with_repo_root() {
        let stage = tempdir();
        let notes = stage.path().join("notes");
        let project_dir = notes.join("test-project");
        std::fs::create_dir_all(&project_dir).unwrap();
        write_item_note(
            &project_dir.join("TST-0001.md"),
            "test-project",
            Some("[handoff]"),
        );

        let repo = stage.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let cfg_path = stage.path().join("config.json");
        write_config(&cfg_path, &notes, "test-project", &repo.to_string_lossy());
        let cfg = crate::config::load(&cfg_path.to_string_lossy(), None).unwrap();

        let result = handoff_gate(&cfg, "TST-0001").unwrap();

        let item = result.expect("tagged item with existing repo should gate to Some");
        assert_eq!(item.id, "TST-0001");
        assert_eq!(item.repo_root, repo);
    }

    #[test]
    fn tagged_item_with_nonexistent_repo_path_errors() {
        let stage = tempdir();
        let notes = stage.path().join("notes");
        let project_dir = notes.join("test-project");
        std::fs::create_dir_all(&project_dir).unwrap();
        write_item_note(
            &project_dir.join("TST-0001.md"),
            "test-project",
            Some("[handoff]"),
        );

        let repo = stage.path().join("does-not-exist");
        let cfg_path = stage.path().join("config.json");
        write_config(&cfg_path, &notes, "test-project", &repo.to_string_lossy());
        let cfg = crate::config::load(&cfg_path.to_string_lossy(), None).unwrap();

        let err = handoff_gate(&cfg, "TST-0001").unwrap_err();

        assert_matches!(
            err,
            MirrorError::RepoRootMissing { ref id, ref path }
                if id == "TST-0001" && path == &repo
        );
    }

    #[test]
    fn tagged_item_with_blank_repo_mapping_errors() {
        let stage = tempdir();
        let notes = stage.path().join("notes");
        let project_dir = notes.join("test-project");
        std::fs::create_dir_all(&project_dir).unwrap();
        write_item_note(
            &project_dir.join("TST-0001.md"),
            "test-project",
            Some("[handoff]"),
        );

        let cfg_path = stage.path().join("config.json");
        write_config(&cfg_path, &notes, "test-project", "");
        let cfg = crate::config::load(&cfg_path.to_string_lossy(), None).unwrap();

        let err = handoff_gate(&cfg, "TST-0001").unwrap_err();

        assert_matches!(
            err,
            MirrorError::UnmanagedProject { ref id, ref project }
                if id == "TST-0001" && project == "test-project"
        );
    }

    #[test]
    fn tagged_item_found_via_archive_still_gates() {
        let stage = tempdir();
        let notes = stage.path().join("notes");
        let archive_dir = notes.join("test-project").join("_archive");
        std::fs::create_dir_all(&archive_dir).unwrap();
        let mut note = String::from(
            "---\nstatus: done\ntitle: test item\nproject: test-project\ncreated: 2026-07-01\ntags: [handoff]\n",
        );
        note.push_str("---\n\nbody\n");
        std::fs::write(archive_dir.join("TST-0001.md"), note).unwrap();

        let repo = stage.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let cfg_path = stage.path().join("config.json");
        write_config(&cfg_path, &notes, "test-project", &repo.to_string_lossy());
        let cfg = crate::config::load(&cfg_path.to_string_lossy(), None).unwrap();

        let result = handoff_gate(&cfg, "TST-0001").unwrap();

        let item = result.expect("archived tagged item should still gate");
        assert_eq!(item.id, "TST-0001");
        assert_eq!(item.repo_root, repo);
    }

    fn write_handoff(dir: &Path, name: &str, content: &str) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, content).unwrap();
        path
    }

    fn gate(id: &str, repo_root: &Path) -> GateItem {
        GateItem {
            id: id.to_string(),
            repo_root: repo_root.to_path_buf(),
        }
    }

    #[test]
    fn set_frontmatter_field_inserts_when_absent() {
        let content = "---\nstatus: active\nproject: p\ncreated: 2026-01-01\n---\n\nbody\n";
        let out = set_frontmatter_field(content, "completed", "2026-01-02");
        assert!(out.contains("status: active\ncompleted: 2026-01-02\n"));
    }

    #[test]
    fn set_frontmatter_field_replaces_when_present() {
        let content = "---\nstatus: active\ncompleted: old\nproject: p\n---\n\nbody\n";
        let out = set_frontmatter_field(content, "completed", "2026-01-02");
        assert!(out.contains("completed: 2026-01-02"));
        assert!(!out.contains("completed: old"));
    }

    #[test]
    fn drop_frontmatter_field_removes_line_without_residue() {
        let content = "---\nstatus: done\ncompleted: 2026-01-02\nproject: p\n---\n\nbody\n";
        let out = drop_frontmatter_field(content, "completed");
        assert!(!out.contains("completed:"), "completed lingered: {out}");
        assert!(
            out.contains("status: done\nproject: p\n"),
            "blank residue: {out}"
        );
    }

    #[test]
    fn preflight_close_done_then_commit_archives_and_rebuilds_ledger() {
        let stage = tempdir();
        let repo = stage.path().join("repo");
        let handoff_dir = repo.join("docs/handoffs");
        write_handoff(
            &handoff_dir,
            "2026-01-01-managed-flow.md",
            "---\nstatus: active\nproject: test-project\ncreated: 2026-01-01\npw: TST-0001\n---\n\n# Managed Flow\n",
        );

        let g = gate("TST-0001", &repo);
        let pending = preflight_close(&g, MirrorClose::Done, "2026-01-02", None)
            .unwrap()
            .expect("active handoff should stage a move");
        let dest = pending.commit().unwrap();

        assert_eq!(
            dest,
            handoff_dir.join("archived/2026-01-01-managed-flow.md")
        );
        let archived = std::fs::read_to_string(&dest).unwrap();
        assert!(archived.contains("status: done"), "got: {archived}");
        assert!(
            archived.contains("completed: 2026-01-02"),
            "got: {archived}"
        );
        assert!(!handoff_dir.join("2026-01-01-managed-flow.md").exists());

        let ledger = std::fs::read_to_string(handoff_dir.join("LEDGER.md")).unwrap();
        assert!(
            !ledger.contains("TST-0001"),
            "closed item should have no ledger row: {ledger}"
        );
    }

    #[test]
    fn preflight_close_cancelled_appends_report_to_body() {
        let stage = tempdir();
        let repo = stage.path().join("repo");
        let handoff_dir = repo.join("docs/handoffs");
        write_handoff(
            &handoff_dir,
            "2026-01-01-managed-flow.md",
            "---\nstatus: active\nproject: test-project\ncreated: 2026-01-01\npw: TST-0001\n---\n\n# Managed Flow\n",
        );

        let g = gate("TST-0001", &repo);
        let pending = preflight_close(&g, MirrorClose::Cancelled, "2026-01-02", Some("obsoleted"))
            .unwrap()
            .expect("active handoff should stage a move");
        let dest = pending.commit().unwrap();

        let archived = std::fs::read_to_string(&dest).unwrap();
        assert!(archived.contains("status: cancelled"), "got: {archived}");
        assert!(
            archived.trim_end().ends_with("> Cancelled: obsoleted"),
            "got: {archived}"
        );
    }

    #[test]
    fn preflight_close_errors_when_no_handoff_links_the_item() {
        let stage = tempdir();
        let repo = stage.path().join("repo");
        std::fs::create_dir_all(repo.join("docs/handoffs")).unwrap();

        let g = gate("TST-0001", &repo);
        let err = preflight_close(&g, MirrorClose::Done, "2026-01-02", None).unwrap_err();

        assert_matches!(err, MirrorError::HandoffNotFound { ref id, .. } if id == "TST-0001");
    }

    #[test]
    fn preflight_close_returns_none_when_handoff_already_archived() {
        let stage = tempdir();
        let repo = stage.path().join("repo");
        let handoff_dir = repo.join("docs/handoffs");
        write_handoff(
            &handoff_dir.join("archived"),
            "2026-01-01-managed-flow.md",
            "---\nstatus: done\ncompleted: 2026-01-02\nproject: test-project\ncreated: 2026-01-01\npw: TST-0001\n---\n\n# Managed Flow\n",
        );

        let g = gate("TST-0001", &repo);
        let pending = preflight_close(&g, MirrorClose::Done, "2026-01-03", None).unwrap();

        assert!(
            pending.is_none(),
            "an already-archived handoff should stage no move"
        );
        assert!(
            handoff_dir
                .join("archived/2026-01-01-managed-flow.md")
                .exists(),
            "archived handoff must be untouched"
        );
    }

    #[test]
    fn preflight_close_errors_when_two_handoffs_claim_the_item_case_insensitively() {
        let stage = tempdir();
        let repo = stage.path().join("repo");
        let handoff_dir = repo.join("docs/handoffs");
        write_handoff(
            &handoff_dir,
            "2026-01-01-first.md",
            "---\nstatus: active\nproject: test-project\ncreated: 2026-01-01\npw: TST-0001\n---\n\n# First\n",
        );
        write_handoff(
            &handoff_dir,
            "2026-01-02-second.md",
            "---\nstatus: active\nproject: test-project\ncreated: 2026-01-02\npw: tst-0001\n---\n\n# Second\n",
        );

        let g = gate("TST-0001", &repo);
        let err = preflight_close(&g, MirrorClose::Done, "2026-01-03", None).unwrap_err();

        assert_matches!(err, MirrorError::AmbiguousHandoff { ref id, .. } if id == "TST-0001");
    }

    #[test]
    fn preflight_close_errors_when_archive_destination_exists() {
        let stage = tempdir();
        let repo = stage.path().join("repo");
        let handoff_dir = repo.join("docs/handoffs");
        write_handoff(
            &handoff_dir,
            "2026-01-01-managed-flow.md",
            "---\nstatus: active\nproject: test-project\ncreated: 2026-01-01\npw: TST-0001\n---\n\n# Managed Flow\n",
        );
        write_handoff(
            &handoff_dir.join("archived"),
            "2026-01-01-managed-flow.md",
            "existing archive\n",
        );

        let g = gate("TST-0001", &repo);
        let err = preflight_close(&g, MirrorClose::Done, "2026-01-02", None).unwrap_err();

        assert_matches!(err, MirrorError::ArchiveAlreadyExists { .. });
    }

    #[test]
    fn preflight_reopen_then_commit_restores_active_without_completed_stamp() {
        let stage = tempdir();
        let repo = stage.path().join("repo");
        let handoff_dir = repo.join("docs/handoffs");
        write_handoff(
            &handoff_dir.join("archived"),
            "2026-01-01-managed-flow.md",
            "---\nstatus: done\ncompleted: 2026-01-02\nproject: test-project\ncreated: 2026-01-01\npw: TST-0001\n---\n\n# Managed Flow\n",
        );
        std::fs::create_dir_all(&handoff_dir).unwrap();
        std::fs::write(handoff_dir.join("LEDGER.md"), "# stale\n").unwrap();

        let g = gate("TST-0001", &repo);
        let pending = preflight_reopen(&g)
            .unwrap()
            .expect("archived handoff should stage a move");
        let dest = pending.commit().unwrap();

        assert_eq!(dest, handoff_dir.join("2026-01-01-managed-flow.md"));
        let active = std::fs::read_to_string(&dest).unwrap();
        assert!(active.contains("status: active"), "got: {active}");
        assert!(!active.contains("completed:"), "got: {active}");
        assert!(
            !handoff_dir
                .join("archived/2026-01-01-managed-flow.md")
                .exists()
        );
    }

    #[test]
    fn preflight_reopen_returns_none_when_pair_is_already_active() {
        let stage = tempdir();
        let repo = stage.path().join("repo");
        let handoff_dir = repo.join("docs/handoffs");
        write_handoff(
            &handoff_dir,
            "2026-01-01-managed-flow.md",
            "---\nstatus: active\nproject: test-project\ncreated: 2026-01-01\npw: TST-0001\n---\n\n# Managed Flow\n",
        );

        let g = gate("TST-0001", &repo);
        let pending = preflight_reopen(&g).unwrap();

        assert!(pending.is_none(), "active pair should stage no move");
    }

    #[test]
    fn preflight_reopen_errors_when_handoff_in_neither_dir() {
        let stage = tempdir();
        let repo = stage.path().join("repo");
        std::fs::create_dir_all(repo.join("docs/handoffs/archived")).unwrap();

        let g = gate("TST-0001", &repo);
        let err = preflight_reopen(&g).unwrap_err();

        assert_matches!(
            err,
            MirrorError::HandoffNotFound { ref id, ref dir }
                if id == "TST-0001" && dir.ends_with("archived")
        );
    }

    #[test]
    fn preflight_reopen_errors_when_active_destination_exists_and_leaves_archive_untouched() {
        let stage = tempdir();
        let repo = stage.path().join("repo");
        let handoff_dir = repo.join("docs/handoffs");
        write_handoff(
            &handoff_dir.join("archived"),
            "2026-01-01-managed-flow.md",
            "---\nstatus: done\ncompleted: 2026-01-02\nproject: test-project\ncreated: 2026-01-01\npw: TST-0001\n---\n\n# Managed Flow\n",
        );
        write_handoff(
            &handoff_dir,
            "2026-01-01-managed-flow.md",
            "---\nstatus: active\nproject: test-project\ncreated: 2026-01-01\npw: TST-0002\n---\n\n# Conflicting\n",
        );

        let g = gate("TST-0001", &repo);
        let err = preflight_reopen(&g).unwrap_err();

        assert_matches!(err, MirrorError::HandoffAlreadyExists { .. });
        assert!(
            handoff_dir
                .join("archived/2026-01-01-managed-flow.md")
                .exists()
        );
    }

    #[test]
    fn delete_for_item_removes_active_file_and_rebuilds_ledger() {
        let stage = tempdir();
        let repo = stage.path().join("repo");
        let handoff_dir = repo.join("docs/handoffs");
        write_handoff(
            &handoff_dir,
            "2026-01-01-managed-flow.md",
            "---\nstatus: active\nproject: test-project\ncreated: 2026-01-01\npw: TST-0001\n---\n\n# Managed Flow\n",
        );
        std::fs::write(handoff_dir.join("LEDGER.md"), "# stale\n").unwrap();

        let g = gate("TST-0001", &repo);
        let removed = delete_for_item(&g).unwrap();

        assert_eq!(removed, handoff_dir.join("2026-01-01-managed-flow.md"));
        assert!(!removed.exists());
        let ledger = std::fs::read_to_string(handoff_dir.join("LEDGER.md")).unwrap();
        assert!(
            !ledger.contains("TST-0001"),
            "deleted item should have no ledger row: {ledger}"
        );
    }

    #[test]
    fn delete_for_item_errors_when_no_handoff_links_the_item() {
        let stage = tempdir();
        let repo = stage.path().join("repo");
        std::fs::create_dir_all(repo.join("docs/handoffs")).unwrap();

        let g = gate("TST-0001", &repo);
        let err = delete_for_item(&g).unwrap_err();

        assert_matches!(err, MirrorError::HandoffNotFound { .. });
    }

    #[test]
    fn preflight_scaffold_then_commit_creates_file_with_pw_and_sections() {
        let stage = tempdir();
        let repo = stage.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let notes = stage.path().join("notes");
        let cfg_path = stage.path().join("config.json");
        write_config(&cfg_path, &notes, "test-project", &repo.to_string_lossy());
        let cfg = crate::config::load(&cfg_path.to_string_lossy(), None).unwrap();

        let pending =
            preflight_scaffold(&cfg, "test-project", "Managed Flow", "2026-01-01").unwrap();
        let path = pending.commit("TST-0001").unwrap();

        assert_eq!(path, repo.join("docs/handoffs/2026-01-01-managed-flow.md"));
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("pw: TST-0001"), "got: {content}");
        assert!(content.contains("# Managed Flow"), "got: {content}");
        assert!(content.contains("## Goals"), "got: {content}");

        let ledger = std::fs::read_to_string(repo.join("docs/handoffs/LEDGER.md")).unwrap();
        assert!(ledger.contains("TST-0001"), "got: {ledger}");
    }

    #[test]
    fn pending_scaffold_commit_rolls_back_file_when_ledger_rebuild_fails() {
        let stage = tempdir();
        let repo = stage.path().join("repo");
        // A directory squatting on the LEDGER path makes the post-write ledger
        // rebuild fail (the atomic temp-then-rename can't replace a directory).
        std::fs::create_dir_all(repo.join("docs/handoffs/LEDGER.md")).unwrap();
        let notes = stage.path().join("notes");
        let cfg_path = stage.path().join("config.json");
        write_config(&cfg_path, &notes, "test-project", &repo.to_string_lossy());
        let cfg = crate::config::load(&cfg_path.to_string_lossy(), None).unwrap();

        let pending =
            preflight_scaffold(&cfg, "test-project", "Managed Flow", "2026-01-01").unwrap();
        let err = pending.commit("TST-0001").unwrap_err();

        assert_matches!(err, MirrorError::Ledger { .. });
        assert!(
            !repo
                .join("docs/handoffs/2026-01-01-managed-flow.md")
                .exists(),
            "scaffold file must be rolled back when the ledger rebuild fails"
        );
    }

    #[test]
    fn preflight_scaffold_errors_when_destination_exists() {
        let stage = tempdir();
        let repo = stage.path().join("repo");
        write_handoff(
            &repo.join("docs/handoffs"),
            "2026-01-01-managed-flow.md",
            "existing\n",
        );
        let notes = stage.path().join("notes");
        let cfg_path = stage.path().join("config.json");
        write_config(&cfg_path, &notes, "test-project", &repo.to_string_lossy());
        let cfg = crate::config::load(&cfg_path.to_string_lossy(), None).unwrap();

        let err =
            preflight_scaffold(&cfg, "test-project", "Managed Flow", "2026-01-01").unwrap_err();

        assert_matches!(err, MirrorError::HandoffAlreadyExists { .. });
    }
}
