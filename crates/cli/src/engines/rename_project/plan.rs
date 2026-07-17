//! Builds immutable rename plans and provides their pure text rewriters.

use std::{
    fmt::Write as _,
    path::{Path, PathBuf},
};

use walkdir::WalkDir;

use super::{RenameContext, RenameProjectError};

/// Identifies a task note by authoritative frontmatter ID.
pub struct ItemFile {
    pub id: String,
    pub path: PathBuf,
}

/// Identifies one post-directory-move note rename.
pub struct FileRename {
    pub from: PathBuf,
    pub to: PathBuf,
}

/// Records ID-token matches in one vault Markdown file.
pub struct TokenHit {
    pub path: PathBuf,
    pub count: usize,
}

/// Contains the immutable inputs for rendering or applying a rename.
/// File rename and label-update paths use post-move locations.
pub struct RenamePlan {
    pub old_code: String,
    pub new_code: String,
    pub old_path: String,
    pub new_path: String,
    pub path_changed: bool,
    pub old_folder: PathBuf,
    pub new_folder: PathBuf,
    pub old_label: String,
    pub new_label: String,
    /// Contains the directory move when the path changes.
    pub dir_move: Option<(PathBuf, PathBuf)>,
    /// Contains the post-move index rename when the basename changes and the old index exists.
    /// The index filename must continue to match the directory basename.
    pub index_rename: Option<(PathBuf, PathBuf)>,
    pub index_identity_update: PathBuf,
    pub file_renames: Vec<FileRename>,
    pub token_hits: Vec<TokenHit>,
    pub label_updates: Vec<PathBuf>,
    /// Lists pre-move destinations that must be absent before a code change.
    pub collision_dests: Vec<PathBuf>,
    pub root: PathBuf,
    pub manifest_path: PathBuf,
}

/// Collects direct child task notes from authoritative frontmatter.
pub fn enumerate_items(
    folder: &Path,
    old_code: &str,
    old_label: &str,
) -> Result<Vec<ItemFile>, pwf_infra::obsidian::ObsidianStoreError> {
    let index = index_path(folder, old_label);
    pwf_infra::obsidian::inspect_project_task_notes(folder, &index, old_code, old_label).map(
        |tasks| {
            tasks
                .into_iter()
                .map(|task| ItemFile {
                    id: task.id.as_ref().to_string(),
                    path: task.path,
                })
                .collect()
        },
    )
}

fn index_path(folder: &Path, basename: &str) -> PathBuf {
    folder.join(format!("{basename}.md"))
}

fn post_move_path(ctx: &RenameContext, path: &Path) -> PathBuf {
    if ctx.path_changed {
        ctx.new_folder.join(
            path.strip_prefix(&ctx.old_folder)
                .expect("inventoried task is in project"),
        )
    } else {
        path.to_path_buf()
    }
}

fn renamed_id(id: &str, old_code: &str, new_code: &str) -> String {
    id.strip_prefix(&format!("{old_code}-"))
        .map_or_else(|| id.to_string(), |number| format!("{new_code}-{number}"))
}

/// Builds the immutable plan and its preflight collision set.
/// Token hits are report-only; apply re-scans the vault before rewriting.
pub fn build_plan(ctx: &RenameContext, items: &[ItemFile]) -> RenamePlan {
    let code_changed = ctx.new_code != ctx.old_code;
    let dir_move = ctx
        .path_changed
        .then(|| (ctx.old_folder.clone(), ctx.new_folder.clone()));

    let file_renames: Vec<_> = items
        .iter()
        .filter(|item| {
            code_changed && item.path.file_stem().and_then(|stem| stem.to_str()) == Some(&item.id)
        })
        .map(|item| {
            let from = post_move_path(ctx, &item.path);
            let to = from.with_file_name(format!(
                "{}.md",
                renamed_id(&item.id, &ctx.old_code, &ctx.new_code)
            ));
            FileRename { from, to }
        })
        .collect();

    // A code change must not overwrite an existing new-prefix note.
    let collision_dests = if code_changed {
        file_renames
            .iter()
            .map(|rename| {
                ctx.old_folder.join(
                    rename
                        .to
                        .strip_prefix(&ctx.new_folder)
                        .unwrap_or(&rename.to),
                )
            })
            .collect()
    } else {
        Vec::new()
    };

    let mut token_hits = Vec::new();
    for entry in WalkDir::new(&ctx.root).into_iter().filter_map(Result::ok) {
        if !is_scannable_md(entry.path(), entry.file_type().is_file()) {
            continue;
        }
        if let Ok(text) = std::fs::read_to_string(entry.path()) {
            let (_, n) = replace_id_tokens(&text, &ctx.old_code, &ctx.new_code);
            if n > 0 {
                token_hits.push(TokenHit {
                    path: entry.path().to_path_buf(),
                    count: n,
                });
            }
        }
    }
    token_hits.sort_by(|a, b| a.path.cmp(&b.path));

    // The project index follows the directory basename when that basename changes.
    let label_changed = ctx.new_label != ctx.old_label;
    let index_rename = (label_changed && index_path(&ctx.old_folder, &ctx.old_label).is_file())
        .then(|| {
            (
                index_path(&ctx.new_folder, &ctx.old_label),
                index_path(&ctx.new_folder, &ctx.new_label),
            )
        });
    let index_identity_update = index_rename.as_ref().map_or_else(
        || index_path(&ctx.new_folder, &ctx.old_label),
        |(_, to)| to.clone(),
    );

    let mut label_updates: Vec<PathBuf> = if ctx.path_changed {
        items
            .iter()
            .map(|item| {
                let moved = post_move_path(ctx, &item.path);
                file_renames
                    .iter()
                    .find(|rename| rename.from == moved)
                    .map_or(moved, |rename| rename.to.clone())
            })
            .collect()
    } else {
        Vec::new()
    };
    if let Some((_, index_to)) = &index_rename {
        label_updates.push(index_to.clone());
    }

    RenamePlan {
        old_code: ctx.old_code.clone(),
        new_code: ctx.new_code.clone(),
        old_path: ctx.old_path.clone(),
        new_path: ctx.new_path.clone(),
        path_changed: ctx.path_changed,
        old_folder: ctx.old_folder.clone(),
        new_folder: ctx.new_folder.clone(),
        old_label: ctx.old_label.clone(),
        new_label: ctx.new_label.clone(),
        dir_move,
        index_rename,
        index_identity_update,
        file_renames,
        token_hits,
        label_updates,
        collision_dests,
        root: ctx.root.clone(),
        manifest_path: ctx.manifest_path.clone(),
    }
}

/// Reports whether a regular Markdown file is outside `.trash`.
pub fn is_scannable_md(path: &Path, is_file: bool) -> bool {
    is_file
        && path.extension().and_then(|e| e.to_str()) == Some("md")
        && !path.components().any(|c| c.as_os_str() == ".trash")
}

/// Rejects plans whose new directory or code-prefixed note destinations already exist.
/// Same-code moves do not collide with notes that retain their basename.
pub fn check_collisions(plan: &RenamePlan) -> Result<(), RenameProjectError> {
    if plan.path_changed && plan.new_folder.exists() {
        return Err(RenameProjectError::Collision(plan.new_folder.clone()));
    }
    for dest in &plan.collision_dests {
        if dest.exists() {
            return Err(RenameProjectError::Collision(dest.clone()));
        }
    }
    Ok(())
}

impl RenamePlan {
    /// Renders every planned change without mutating the filesystem.
    #[must_use]
    pub fn render(&self) -> String {
        let mut s = String::new();
        let _ = writeln!(s, "rename-project plan (DRY RUN — no changes written)");
        let _ = writeln!(s, "  code:  {} -> {}", self.old_code, self.new_code);
        if self.path_changed {
            let _ = writeln!(s, "  path:  {} -> {}", self.old_path, self.new_path);
            let _ = writeln!(s, "\nDirectory move:");
            let _ = writeln!(
                s,
                "  {} -> {}",
                self.old_folder.display(),
                self.new_folder.display()
            );
        } else {
            let _ = writeln!(s, "  path:  {} (unchanged)", self.old_path);
        }

        let _ = writeln!(s, "\nFile renames ({}):", self.file_renames.len());
        for rename in &self.file_renames {
            let _ = writeln!(
                s,
                "  {} -> {}",
                file_name(&rename.from),
                file_name(&rename.to)
            );
        }

        let _ = writeln!(s, "\nIndex file rename:");
        match &self.index_rename {
            Some((from, to)) => {
                let _ = writeln!(s, "  {} -> {}", file_name(from), file_name(to));
            }
            None => {
                let _ = writeln!(s, "  (none)");
            }
        }

        let _ = writeln!(
            s,
            "\nId-token rewrites across the vault ({} files):",
            self.token_hits.len()
        );
        for hit in &self.token_hits {
            let rel = hit.path.strip_prefix(&self.root).unwrap_or(&hit.path);
            let _ = writeln!(
                s,
                "  {} ({}x [[{}-NNNN]] -> [[{}-NNNN]])",
                rel.display(),
                hit.count,
                self.old_code,
                self.new_code
            );
        }

        if self.path_changed {
            let _ = writeln!(
                s,
                "\nproject: label updates ({} notes): {} -> {}",
                self.label_updates.len(),
                self.old_label,
                self.new_label
            );
        } else {
            let _ = writeln!(s, "\nproject: label unchanged (code-only rename)");
        }

        let _ = writeln!(s, "\nManifest edit ({}):", self.manifest_path.display());
        let _ = writeln!(
            s,
            "  [[repo]] code = \"{}\" -> \"{}\"",
            self.old_code, self.new_code
        );
        if self.path_changed {
            let _ = writeln!(
                s,
                "  [[repo]] path = \"{}\" -> \"{}\"",
                self.old_path, self.new_path
            );
        }
        s
    }

    /// Renders the one-line summary for an applied rename.
    #[must_use]
    pub fn render_summary(&self) -> String {
        let index = usize::from(self.index_rename.is_some());
        format!(
            "rename-project done: {} -> {} ({} files renamed, {index} index renamed, {} vault files re-linked, manifest updated)",
            self.old_code,
            self.new_code,
            self.file_renames.len(),
            self.token_hits.len()
        )
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map_or_else(|| path.to_string_lossy(), |n| n.to_string_lossy())
        .into_owned()
}

/// Replaces whole `<old_code>-<digits>` tokens while preserving their number.
/// Boundaries exclude alphanumeric prefixes, `-` prefixes, and alphanumeric suffixes.
#[must_use]
pub fn replace_id_tokens(content: &str, old_code: &str, new_code: &str) -> (String, usize) {
    let needle = format!("{old_code}-");
    let bytes = content.as_bytes();
    let mut out = String::with_capacity(content.len());
    let mut count = 0usize;
    let mut i = 0usize;
    while let Some(rel) = content[i..].find(&needle) {
        let start = i + rel;
        out.push_str(&content[i..start]);
        let prev_ok = start == 0
            || content[..start]
                .chars()
                .next_back()
                .is_some_and(|c| !c.is_ascii_alphanumeric() && c != '-');
        let digits_start = start + needle.len();
        let mut j = digits_start;
        while j < bytes.len() && bytes[j].is_ascii_digit() {
            j += 1;
        }
        let has_digits = j > digits_start;
        let next_ok = j >= bytes.len() || !bytes[j].is_ascii_alphanumeric();
        if prev_ok && has_digits && next_ok {
            out.push_str(new_code);
            out.push('-');
            out.push_str(&content[digits_start..j]);
            count += 1;
            i = j;
        } else {
            out.push_str(&needle);
            i = digits_start;
        }
    }
    out.push_str(&content[i..]);
    (out, count)
}

/// Rewrites the first matching `project:` value in leading fenced frontmatter.
/// Returns `None` when no value changes.
#[must_use]
pub fn replace_project_label(content: &str, old_label: &str, new_label: &str) -> Option<String> {
    let mut seen_open = false;
    let mut in_frontmatter = false;
    let mut changed = false;
    let mut out = String::with_capacity(content.len());
    for line in content.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\n', '\r']);
        if trimmed == "---" {
            if seen_open {
                in_frontmatter = false;
            } else {
                seen_open = true;
                in_frontmatter = true;
            }
            out.push_str(line);
            continue;
        }
        if in_frontmatter
            && !changed
            && let Some(rest) = trimmed.strip_prefix("project:")
            && rest.trim() == old_label
        {
            let newline = if line.ends_with('\n') { "\n" } else { "" };
            let _ = write!(out, "project: {new_label}{newline}");
            changed = true;
            continue;
        }
        out.push_str(line);
    }
    changed.then_some(out)
}

/// Rewrites authoritative project-index identity fields in frontmatter.
#[must_use]
pub fn replace_project_index_identity(content: &str, new_code: &str, new_label: &str) -> String {
    let mut in_frontmatter = false;
    let mut fence_count = 0;
    let mut out = String::with_capacity(content.len());
    for line in content.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\n', '\r']);
        if trimmed == "---" {
            fence_count += 1;
            in_frontmatter = fence_count == 1;
            out.push_str(line);
            continue;
        }
        let newline = if line.ends_with('\n') { "\n" } else { "" };
        if in_frontmatter && trimmed.starts_with("id:") {
            let _ = write!(out, "id: {}{newline}", new_code.to_ascii_lowercase());
        } else if in_frontmatter && trimmed.starts_with("title:") {
            let _ = write!(out, "title: {new_label}{newline}");
        } else {
            out.push_str(line);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_wikilink_and_bare_tokens() {
        let (out, n) = replace_id_tokens("see [[CFG-0007]] and CFG-0012 done", "CFG", "ARC");
        assert_eq!(out, "see [[ARC-0007]] and ARC-0012 done");
        assert_eq!(n, 2);
    }

    #[test]
    fn preserves_number_and_ignores_note_ids_and_substrings() {
        let (out, n) = replace_id_tokens("CFG-0007 CFG-NOTE-0001 XCFG-0003", "CFG", "ARC");
        assert_eq!(out, "ARC-0007 CFG-NOTE-0001 XCFG-0003");
        assert_eq!(n, 1);
    }

    #[test]
    fn ignores_trailing_letter_and_no_digits() {
        let (out, n) = replace_id_tokens("CFG-0007x CFG- CFG", "CFG", "ARC");
        assert_eq!(out, "CFG-0007x CFG- CFG");
        assert_eq!(n, 0);
    }

    #[test]
    fn label_updates_only_frontmatter_matching_old() {
        let note =
            "---\nstatus: active\nproject: config-handler\n---\n\nbody project: config-handler\n";
        let out = replace_project_label(note, "config-handler", "repository").unwrap();
        assert_eq!(
            out,
            "---\nstatus: active\nproject: repository\n---\n\nbody project: config-handler\n"
        );
    }

    #[test]
    fn label_no_change_returns_none() {
        let note = "---\nproject: other\n---\n";
        assert!(replace_project_label(note, "config-handler", "repository").is_none());
    }
}
