//! `repos.toml` resolution and scoped block editing. The manifest is *parsed*
//! (via `toml`) only to read a code's `path`; edits are a line-scoped rewrite of
//! the single target `[[repo]]` table's `path`/`code` lines, so key order,
//! spacing, comments, and sibling tables survive byte-for-byte (a full-file
//! re-serialize would reformat the whole manifest).

use std::path::PathBuf;

use serde::Deserialize;

#[derive(Deserialize)]
struct Manifest {
    #[serde(default)]
    repo: Vec<Repo>,
}

#[derive(Deserialize)]
struct Repo {
    path: String,
    code: Option<String>,
}

/// The repo-relative `path` of the `[[repo]]` whose `code` matches (ASCII
/// case-insensitive). `None` if no such entry, or the manifest does not parse.
#[must_use]
pub fn resolve_repo_path(manifest: &str, code: &str) -> Option<String> {
    let parsed: Manifest = toml::from_str(manifest).ok()?;
    parsed
        .repo
        .into_iter()
        .find(|r| {
            r.code
                .as_deref()
                .is_some_and(|c| c.eq_ignore_ascii_case(code))
        })
        .map(|r| r.path)
}

/// Rewrite only the `path`/`code` lines inside the single `[[repo]]` table whose
/// `code` matches `old_code`. Everything else — key order, spacing, sibling
/// tables — is preserved byte-for-byte. `path` changes only when `new_path` is
/// `Some`. `None` if the code is not found.
#[must_use]
pub fn edit_manifest_block(
    manifest: &str,
    old_code: &str,
    new_code: &str,
    new_path: Option<&str>,
) -> Option<String> {
    let lines: Vec<&str> = manifest.split_inclusive('\n').collect();
    let headers: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.trim_end_matches(['\n', '\r']).trim() == "[[repo]]")
        .map(|(i, _)| i)
        .collect();

    let (start, end) = headers.iter().enumerate().find_map(|(k, &start)| {
        let end = headers.get(k + 1).copied().unwrap_or(lines.len());
        let matches = lines[start..end]
            .iter()
            .filter_map(|l| key_value(l, "code"))
            .any(|c| c.eq_ignore_ascii_case(old_code));
        matches.then_some((start, end))
    })?;

    let mut out = String::with_capacity(manifest.len());
    for (i, line) in lines.iter().enumerate() {
        if (start..end).contains(&i) {
            if key_value(line, "code").is_some() {
                out.push_str(&rewrite_kv(line, "code", new_code));
                continue;
            }
            if let Some(new_path) = new_path
                && key_value(line, "path").is_some()
            {
                out.push_str(&rewrite_kv(line, "path", new_path));
                continue;
            }
        }
        out.push_str(line);
    }
    Some(out)
}

/// If `line` is `<key> = "<value>"` (ignoring leading whitespace), return `<value>`.
fn key_value<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let rest = line.trim_start().strip_prefix(key)?.trim_start();
    let rest = rest.strip_prefix('=')?.trim_start();
    let inner = rest.strip_prefix('"')?;
    inner.find('"').map(|end| &inner[..end])
}

/// Rebuild a `<key> = "<newval>"` line, preserving the original leading indent
/// and trailing newline.
fn rewrite_kv(line: &str, key: &str, newval: &str) -> String {
    let indent: String = line
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect();
    let newline = if line.ends_with('\n') { "\n" } else { "" };
    format!("{indent}{key} = \"{newval}\"{newline}")
}

/// Default repos.toml location: `$ARCA_ROOT/repos.toml`, else
/// `$HOME/tools/repository/repos.toml` (mirrors repository's asset resolution).
#[must_use]
pub fn default_manifest_path() -> PathBuf {
    if let Ok(root) = std::env::var("ARCA_ROOT")
        && !root.is_empty()
    {
        return PathBuf::from(root).join("repos.toml");
    }
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_default();
    PathBuf::from(home)
        .join("tools")
        .join("repository")
        .join("repos.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
[[repo]]
path = \"self/config-handler\"
remote = \"git@github.com:me/config-handler.git\"
code = \"CFG\"
color = \"#ef7c1a\"

[[repo]]
path = \"self/wine-curation\"
remote = \"git@github.com:me/wine-curation.git\"
code = \"WNE\"
";

    #[test]
    fn resolves_path_by_code() {
        assert_eq!(
            resolve_repo_path(SAMPLE, "CFG").as_deref(),
            Some("self/config-handler")
        );
        assert_eq!(
            resolve_repo_path(SAMPLE, "cfg").as_deref(),
            Some("self/config-handler")
        );
        assert_eq!(resolve_repo_path(SAMPLE, "ZZZ"), None);
    }

    #[test]
    fn edit_rewrites_only_target_block_path_and_code() {
        let out = edit_manifest_block(SAMPLE, "CFG", "ARC", Some("self/repository")).unwrap();
        assert!(out.contains("path = \"self/repository\""));
        assert!(out.contains("code = \"ARC\""));
        assert!(out.contains("path = \"self/wine-curation\""));
        assert!(out.contains("color = \"#ef7c1a\""));
        assert!(out.contains("remote = \"git@github.com:me/config-handler.git\""));
        assert!(!out.contains("code = \"CFG\""));
        assert!(!out.contains("self/config-handler"));
    }

    #[test]
    fn edit_code_only_leaves_path() {
        let out = edit_manifest_block(SAMPLE, "CFG", "ARC", None).unwrap();
        assert!(out.contains("path = \"self/config-handler\""));
        assert!(out.contains("code = \"ARC\""));
    }

    #[test]
    fn edit_unknown_code_returns_none() {
        assert!(edit_manifest_block(SAMPLE, "ZZZ", "ARC", None).is_none());
    }
}
