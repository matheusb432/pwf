//! Architecture gates — mechanical checks over the workspace source trees.
//!
//! Trees are loaded once (one walk + one read per tree) and shared by every check that targets
//! them; checks themselves are pure `&[SourceFile] -> Vec<Violation>` functions, so they are
//! unit-tested without touching the filesystem. Gate 1 is infra containment; gate 2 is
//! no-rendering-in-application; gate 3 is direct-call shape (this task) — it reuses the
//! already-loaded `cli_files` tree, never re-walking.

use std::{fs, io, path::Path};

/// Import identifier scanned for outside the composition roots.
const INFRA_CRATE_NAME: &str = "pwf_infra";
const CLI_SOURCE_ROOT: &str = "crates/cli/src";
const APPLICATION_SOURCE_ROOT: &str = "crates/application/src";

/// Composition roots — permanently allowed to name infra types.
const INFRA_IMPORT_ALLOWLIST_ROOTS: &[&str] = &[
    "crates/cli/src/main.rs",
    "crates/cli/src/engines/pending_work/run.rs",
    "crates/cli/src/engines/handoff/run.rs",
];

/// Exceptions:
/// - query.rs / actions/done.rs / actions/add.rs: downcast the boxed `AppDbStore::Error` to the
///   concrete `ObsidianStoreError` to preserve exact legacy error text (e.g.
///   `NotesDirectoryNotFound`, `AmbiguousId`) the generic port's typed errors don't carry. Phase 4
///   (PWF-0123) audited this after the phase-3 error relocation and confirmed the need is
///   permanent, not transitional — `AppDbStore::Error` is a generic associated type, so a CLI
///   caller that needs infra-specific variants has no seam but downcasting the boxed source.
/// - `engines/rename_project`/*: separate engine outside the PWF-0123 port scope.
const INFRA_IMPORT_ALLOWLIST_EXCEPTIONS: &[&str] = &[
    "crates/cli/src/engines/pending_work/query.rs",
    "crates/cli/src/engines/pending_work/actions/done.rs",
    "crates/cli/src/engines/pending_work/actions/add.rs",
    "crates/cli/src/engines/rename_project/mod.rs",
    "crates/cli/src/engines/rename_project/plan.rs",
];

/// A `*.rs` file loaded from one of the checked trees.
pub(crate) struct SourceFile {
    pub(crate) relative_path: String,
    pub(crate) content: String,
}

/// One check failure: where it was found and why it's disallowed.
pub(crate) struct Violation {
    pub(crate) relative_path: String,
    pub(crate) line: usize,
    pub(crate) message: String,
}

/// Runs every architecture gate over the workspace's source trees, loading each tree once.
///
/// `Ok(Err(violations))` is the check-failed outcome (data, not an I/O error); the outer
/// `io::Result` is reserved for a tree that could not be walked/read at all — that propagates
/// instead of being silently treated as "no violations".
pub(crate) fn run(repo_root: &Path) -> io::Result<Result<(), Vec<Violation>>> {
    let cli_files = load_source_tree(repo_root, CLI_SOURCE_ROOT)?;
    let application_files = load_source_tree(repo_root, APPLICATION_SOURCE_ROOT)?;

    let violations: Vec<Violation> = check_infra_containment(&cli_files)
        .into_iter()
        .chain(check_application_rendering(&application_files))
        .chain(check_direct_call_shape(&cli_files))
        .collect();
    Ok(if violations.is_empty() {
        Ok(())
    } else {
        Err(violations)
    })
}

/// Every infra import outside the allowlisted composition roots.
fn check_infra_containment(files: &[SourceFile]) -> Vec<Violation> {
    files
        .iter()
        .filter(|file| !is_infra_import_allowed(&file.relative_path))
        .flat_map(|file| occurrences(file, INFRA_CRATE_NAME, "outside composition root"))
        .collect()
}

fn is_infra_import_allowed(relative_path: &str) -> bool {
    INFRA_IMPORT_ALLOWLIST_ROOTS.contains(&relative_path)
        || INFRA_IMPORT_ALLOWLIST_EXCEPTIONS.contains(&relative_path)
}

/// Gate 3: an application operation is always called fully qualified —
/// `pwf_application::<path>::execute(...)` — never through an imported/aliased handle. A
/// fully-qualified call needs no `use` of any module at all, so the gate is on imports: any
/// `use pwf_application ...` statement that brings a *module* into scope (a lowercase imported
/// leaf — bare, grouped, `self`, `as`-renamed, or a glob) enables a shortened `add::execute(...)`
/// call and is a violation. Uppercase type/DTO imports (commands, outcomes, error types) stay
/// unflagged.
fn check_direct_call_shape(files: &[SourceFile]) -> Vec<Violation> {
    files
        .iter()
        .filter(|file| file.relative_path.contains("engines/"))
        .flat_map(direct_call_shape_violations_in_file)
        .collect()
}

const USE_APPLICATION_MARKER: &str = "use pwf_application";

fn direct_call_shape_violations_in_file(file: &SourceFile) -> Vec<Violation> {
    use_application_statement_ranges(&file.content)
        .into_iter()
        .filter_map(|(start, end)| {
            let statement = &file.content[start..end];
            direct_call_shape_violation_message(statement).map(|message| Violation {
                relative_path: file.relative_path.clone(),
                line: newline_count_before(&file.content, start) + 1,
                message,
            })
        })
        .collect()
}

/// Byte ranges of every `use pwf_application ... ;` statement (marker through the terminating
/// `;` — a `use` statement never contains a `;` inside its braces, so the first one found always
/// closes it).
fn use_application_statement_ranges(content: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut search_from = 0;
    while let Some(marker_offset) = content[search_from..]
        .find(USE_APPLICATION_MARKER)
        .map(|rel| search_from + rel)
    {
        let end = content[marker_offset..]
            .find(';')
            .map_or(content.len(), |rel| marker_offset + rel + 1);
        ranges.push((marker_offset, end));
        search_from = end;
    }
    ranges
}

/// The violation message for one `use pwf_application ...;` statement, or `None` if it only
/// names types/DTOs (the allowed shape).
fn direct_call_shape_violation_message(statement: &str) -> Option<String> {
    if statement.contains('*') {
        return Some(
            "glob-imports from pwf_application instead of calling `execute` fully qualified"
                .to_string(),
        );
    }
    first_lowercase_imported_leaf(statement).map(|leaf| {
        if leaf == "execute" {
            "imports `execute` instead of calling it fully qualified".to_string()
        } else {
            format!(
                "imports application module `{leaf}` instead of calling `execute` fully qualified"
            )
        }
    })
}

/// The first *module* name `statement` binds into scope: an imported leaf — an identifier
/// followed by `,`, `}`, `;`, `as`, or the statement's end, i.e. not a `::` path prefix — that
/// starts lowercase (`add`, `self`, an imported `execute` fn), at any brace-group nesting depth.
/// Uppercase leaves are types/DTOs and allowed; an `as` alias is judged by the identifier it
/// renames, never by the alias name itself.
fn first_lowercase_imported_leaf(statement: &str) -> Option<&str> {
    let mut previous_token: Option<&str> = None;
    for (offset, token) in identifier_tokens(statement) {
        let renames_previous = previous_token == Some("as");
        previous_token = Some(token);
        if renames_previous || token == "use" || token == "as" {
            continue;
        }
        let rest = statement[offset + token.len()..].trim_start();
        let is_leaf = rest.is_empty()
            || rest.starts_with([',', '}', ';'])
            || rest
                .strip_prefix("as")
                .is_some_and(|after| is_identifier_boundary(after.chars().next()));
        if is_leaf && token.starts_with(|ch: char| ch.is_lowercase()) {
            return Some(token);
        }
    }
    None
}

/// `(offset, identifier)` for every maximal identifier-character run in `statement`, in order.
fn identifier_tokens(statement: &str) -> Vec<(usize, &str)> {
    let is_ident = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_';
    let bytes = statement.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if is_ident(bytes[index]) {
            let start = index;
            while index < bytes.len() && is_ident(bytes[index]) {
                index += 1;
            }
            tokens.push((start, &statement[start..index]));
        } else {
            index += 1;
        }
    }
    tokens
}

/// Macros that build presentation text — application returns typed data for the CLI to render,
/// it does not compose the rendered lines itself.
const RENDERING_MACROS: &[&str] = &[
    "format!(",
    "write!(",
    "writeln!(",
    "println!(",
    "eprintln!(",
];

/// `(file, function)` pairs allowed to call a rendering macro anyway. Two tiers, each entry's
/// comment says which: most compose a plain data value — a spawned task's prompt, an id or
/// frontmatter value — that the CLI still renders (content ≠ presentation); two are genuine
/// presentation strings frozen verbatim by the byte-identical gate, carried as named debt until
/// a follow-up converts them to typed data + CLI render.
const RENDERING_ALLOWLIST: &[(&str, &str)] = &[
    // Builds the spawned `## Human` review task's title/goal text (task *content*, handed to
    // `add::execute` as a new item's prompt) for `pwf done --review`, not a rendered CLI line.
    (
        "crates/application/src/pending_work/done.rs",
        "review_task_prompt",
    ),
    // Dedups/joins raw `--commits` values into the `commits:` frontmatter value. Contains no
    // `format!` today (plain `.join`); listed for allowlist-doc parity with its sibling above,
    // since both exist for the same reason (content ≠ presentation).
    (
        "crates/application/src/pending_work/done.rs",
        "frontmatter_value",
    ),
    // "Work-item note missing: {path}" lands in `issues: Vec<String>`, which `pwf list --long`
    // prints verbatim as `issue: <text>` — presentation, frozen by byte-identical gate; debt:
    // convert to typed Issue data + CLI render.
    (
        "crates/application/src/pending_work/enrich.rs",
        "derive_flags",
    ),
    // Composes the legacy inline record's `<project>:<ordinal>` identity string — an id value,
    // not display text.
    (
        "crates/application/src/pending_work/enrich.rs",
        "into_open_item",
    ),
    // Same inline `<project>:<ordinal>` id composition, used only to match an incoming id string.
    (
        "crates/application/src/pending_work/resolve.rs",
        "resolve_inline_record",
    ),
    // "commits: <range>" joins `changes: Vec<String>` (a display-message list — its sibling entry
    // is the pure-English "report appended") rendered under `Updated pwf task:` — presentation,
    // frozen by byte-identical gate; debt: convert to typed change data + CLI render.
    (
        "crates/application/src/pending_work/update.rs",
        "amend_closed_item",
    ),
    // "[[<ID>]]" is the literal `prereq:` frontmatter value being composed, not a message.
    (
        "crates/application/src/pending_work/update.rs",
        "merge_prereqs",
    ),
];

/// Every rendering-macro call in application, outside a test/error/allowlisted context.
///
/// A file entirely gated by an external `#[cfg(test)] mod <name>;` declaration (e.g. `lib.rs`'s
/// `#[cfg(test)] mod testing;`, backed by `testing.rs`) never ships in the production binary, so
/// it is skipped outright rather than scanned line by line.
fn check_application_rendering(files: &[SourceFile]) -> Vec<Violation> {
    let test_only = test_only_module_files(files);
    files
        .iter()
        .filter(|file| !test_only.contains(&file.relative_path))
        .flat_map(rendering_violations_in_file)
        .collect()
}

/// Relative paths of files that back a `#[cfg(test)]\nmod <name>;` external module declaration
/// found anywhere in the tree — the whole file is test-only, regardless of its own content.
fn test_only_module_files(files: &[SourceFile]) -> Vec<String> {
    let names: Vec<String> = files
        .iter()
        .flat_map(|file| external_test_only_module_names(&file.content))
        .collect();
    files
        .iter()
        .filter(|file| {
            names
                .iter()
                .any(|name| backs_module(&file.relative_path, name))
        })
        .map(|file| file.relative_path.clone())
        .collect()
}

/// Module names declared as `#[cfg(test)]\nmod <name>;` (semicolon form — an external file, not
/// an inline body) anywhere in `content`.
fn external_test_only_module_names(content: &str) -> Vec<String> {
    let lines: Vec<&str> = content.lines().collect();
    lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.trim() == "#[cfg(test)]")
        .filter_map(|(i, _)| lines.get(i + 1))
        .filter_map(|next| next.trim().strip_prefix("mod "))
        .filter_map(|rest| rest.trim().strip_suffix(';'))
        .map(str::to_string)
        .collect()
}

/// Whether `relative_path` is the file backing module `name` (`<name>.rs` or `<name>/mod.rs`).
fn backs_module(relative_path: &str, name: &str) -> bool {
    relative_path.ends_with(&format!("/{name}.rs"))
        || relative_path.ends_with(&format!("/{name}/mod.rs"))
}

/// Rendering-macro violations in one file, after masking test/error/allowlisted spans.
fn rendering_violations_in_file(file: &SourceFile) -> Vec<Violation> {
    let mut masked = cfg_test_mod_body_ranges(&file.content);
    masked.extend(error_attribute_ranges(&file.content));
    masked.extend(allowlisted_function_ranges(file));

    RENDERING_MACROS
        .iter()
        .flat_map(|needle| {
            file.content
                .match_indices(needle)
                .filter(|(offset, _)| is_macro_call_start(&file.content, *offset))
                .filter(|(offset, _)| {
                    !masked
                        .iter()
                        .any(|&(start, end)| (start..end).contains(offset))
                })
                .map(|(offset, _)| Violation {
                    relative_path: file.relative_path.clone(),
                    line: newline_count_before(&file.content, offset) + 1,
                    message: format!("{needle} builds presentation text in application"),
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Whether `offset` starts a real macro-name token rather than landing mid-identifier — e.g.
/// `"println!("` is a substring of `"eprintln!("`, so a plain substring match alone would
/// double-count the latter.
fn is_macro_call_start(content: &str, offset: usize) -> bool {
    is_identifier_boundary(content[..offset].chars().next_back())
}

/// Whether `ch` (or its absence, at either end of the string) can bound a plain identifier —
/// i.e. it is not itself an identifier character. Shared by every substring scan here that must
/// tell a whole-word match (the `as` keyword, a macro name) from one landing mid-identifier
/// (`assert`, `eprintln!`).
fn is_identifier_boundary(ch: Option<char>) -> bool {
    ch.is_none_or(|ch| !(ch.is_alphanumeric() || ch == '_'))
}

/// Byte ranges of every inline `#[cfg(test)]\nmod <name> { ... }` body (brace-tracked from the
/// marker), covering the marker itself through the matching closing brace.
fn cfg_test_mod_body_ranges(content: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut search_from = 0;
    while let Some(marker_offset) = content[search_from..]
        .find("#[cfg(test)]")
        .map(|rel| search_from + rel)
    {
        match following_mod_block_open_brace(content, marker_offset) {
            Some(open) => {
                let close = matching_delimiter_close(content, open, b'{', b'}');
                ranges.push((marker_offset, close + 1));
                search_from = close + 1;
            }
            None => search_from = marker_offset + "#[cfg(test)]".len(),
        }
    }
    ranges
}

/// The offset of the `{` opening an inline `mod <name> { ... }` body declared on the line right
/// after `marker_offset`, or `None` if that line isn't an inline mod-body declaration at all —
/// an external `mod <name>;` (handled separately) or, just as in `crates/cli` (e.g.
/// `#[cfg(test)] pub(crate) use run::store_for;`), an unrelated cfg-gated item. Bounded to that
/// one line so an unrelated marker can never "capture" some unrelated, far-later `mod tests {`
/// via an unbounded forward search.
fn following_mod_block_open_brace(content: &str, marker_offset: usize) -> Option<usize> {
    let next_line_start = marker_offset + content[marker_offset..].find('\n')? + 1;
    let next_line_len = content[next_line_start..]
        .find('\n')
        .unwrap_or(content.len() - next_line_start);
    let next_line = &content[next_line_start..next_line_start + next_line_len];
    let (_, after_mod) = next_line.split_once("mod ")?;
    after_mod
        .trim_end()
        .ends_with('{')
        .then(|| next_line_start + next_line.rfind('{').expect("checked ends_with '{' above"))
}

/// Byte ranges of every `#[error(...)]` attribute (paren-tracked, so a multi-line
/// `#[error(\n    "..."\n)]` form is covered too) — thiserror `Display` strings are permitted.
fn error_attribute_ranges(content: &str) -> Vec<(usize, usize)> {
    const MARKER: &str = "#[error(";
    let mut ranges = Vec::new();
    let mut search_from = 0;
    while let Some(marker_offset) = content[search_from..]
        .find(MARKER)
        .map(|rel| search_from + rel)
    {
        let open_paren = marker_offset + MARKER.len() - 1;
        let close_paren = matching_delimiter_close(content, open_paren, b'(', b')');
        let end = content[close_paren..]
            .find(']')
            .map_or(close_paren + 1, |rel| close_paren + rel + 1);
        ranges.push((marker_offset, end));
        search_from = end;
    }
    ranges
}

/// Byte ranges of every allowlisted function's body in `file`, keyed by `(file, function)` pairs
/// in [`RENDERING_ALLOWLIST`] matching `file.relative_path`.
fn allowlisted_function_ranges(file: &SourceFile) -> Vec<(usize, usize)> {
    RENDERING_ALLOWLIST
        .iter()
        .filter(|(path, _)| *path == file.relative_path)
        .filter_map(|(_, name)| function_body_range(&file.content, name))
        .collect()
}

/// The byte range of `fn <name>(...) { ... }`'s body (brace-tracked), from the `fn` keyword
/// through the matching closing brace, or `None` if `name` is not defined in `content`. Tolerates
/// generics/where-clauses between the name and the opening brace (`fn foo<S>(..) -> T where .. {`).
fn function_body_range(content: &str, name: &str) -> Option<(usize, usize)> {
    let marker = format!("fn {name}");
    let mut search_from = 0;
    loop {
        let start = search_from + content[search_from..].find(&marker)?;
        let after_name = start + marker.len();
        let boundary_ok = content[after_name..]
            .chars()
            .next()
            .is_none_or(|ch| !(ch.is_alphanumeric() || ch == '_'));
        if boundary_ok {
            let open = after_name + content[after_name..].find('{')?;
            let close = matching_delimiter_close(content, open, b'{', b'}');
            return Some((start, close + 1));
        }
        search_from = after_name;
    }
}

/// The offset of the delimiter matching the one at `open` (which must hold `open_byte`), tracking
/// nesting depth over raw bytes — safe even across UTF-8 text, since a multi-byte character's
/// continuation bytes never collide with an ASCII delimiter byte.
fn matching_delimiter_close(content: &str, open: usize, open_byte: u8, close_byte: u8) -> usize {
    let bytes = content.as_bytes();
    debug_assert_eq!(bytes[open], open_byte);
    let mut depth: i32 = 0;
    for (i, &byte) in bytes.iter().enumerate().skip(open) {
        if byte == open_byte {
            depth += 1;
        } else if byte == close_byte {
            depth -= 1;
            if depth == 0 {
                return i;
            }
        }
    }
    bytes.len().saturating_sub(1)
}

/// Whole-content substring scan; line numbers computed only for hits.
fn occurrences(file: &SourceFile, needle: &str, why: &str) -> Vec<Violation> {
    file.content
        .match_indices(needle)
        .map(|(offset, _)| Violation {
            relative_path: file.relative_path.clone(),
            line: newline_count_before(&file.content, offset) + 1,
            message: format!("{needle} {why}"),
        })
        .collect()
}

// The corpus is <1 MB and this only runs on match hits, so a `bytecount`-crate dependency buys
// nothing here — plain iteration is fine.
#[allow(clippy::naive_bytecount)]
fn newline_count_before(content: &str, offset: usize) -> usize {
    content.as_bytes()[..offset]
        .iter()
        .filter(|&&byte| byte == b'\n')
        .count()
}

/// One walk + one read per tree, shared by every check that targets it. `relative_path` is
/// repo-relative with `/` separators (stable across Windows/Linux checkouts).
fn load_source_tree(repo_root: &Path, source_root: &str) -> io::Result<Vec<SourceFile>> {
    let mut files = Vec::new();
    walk_rs_files(repo_root, &repo_root.join(source_root), &mut files)?;
    files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    Ok(files)
}

fn walk_rs_files(repo_root: &Path, dir: &Path, files: &mut Vec<SourceFile>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            walk_rs_files(repo_root, &path, files)?;
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            let content = fs::read_to_string(&path)?;
            files.push(SourceFile {
                relative_path: relative_slash_path(repo_root, &path),
                content,
            });
        }
    }
    Ok(())
}

/// `path` made repo-relative with forward-slash separators, regardless of host OS.
fn relative_slash_path(repo_root: &Path, path: &Path) -> String {
    path.strip_prefix(repo_root)
        .unwrap_or(path)
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source_file(relative_path: &str, content: &str) -> SourceFile {
        SourceFile {
            relative_path: relative_path.to_string(),
            content: content.to_string(),
        }
    }

    #[test]
    fn flags_infra_import_outside_the_allowlist_with_the_right_line() {
        let files = [source_file(
            "crates/cli/src/engines/pending_work/actions/remove.rs",
            "use pwf_domain::WorkItemId;\nuse pwf_infra::obsidian::ObsidianStore;\n",
        )];

        let violations = check_infra_containment(&files);

        assert_eq!(violations.len(), 1);
        assert_eq!(
            violations[0].relative_path,
            "crates/cli/src/engines/pending_work/actions/remove.rs"
        );
        assert_eq!(violations[0].line, 2);
        assert_eq!(violations[0].message, "pwf_infra outside composition root");
    }

    #[test]
    fn allowlisted_composition_root_is_not_flagged() {
        let files = [source_file(
            "crates/cli/src/main.rs",
            "use pwf_infra::obsidian::ObsidianStore;\n",
        )];

        assert!(check_infra_containment(&files).is_empty());
    }

    #[test]
    fn allowlisted_exception_is_not_flagged() {
        let files = [source_file(
            "crates/cli/src/engines/pending_work/query.rs",
            "use pwf_infra::obsidian::ObsidianStoreError;\n",
        )];

        assert!(check_infra_containment(&files).is_empty());
    }

    #[test]
    fn one_violation_per_occurrence_in_a_single_file() {
        let files = [source_file(
            "crates/cli/src/engines/pending_work/actions/weird.rs",
            "use pwf_infra::a;\nuse pwf_infra::b;\nuse pwf_infra::c;\n",
        )];

        let violations = check_infra_containment(&files);

        assert_eq!(violations.len(), 3);
        assert_eq!(
            violations.iter().map(|v| v.line).collect::<Vec<_>>(),
            [1, 2, 3]
        );
    }

    #[test]
    fn clean_file_yields_no_violations() {
        let files = [source_file(
            "crates/cli/src/engines/pending_work/actions/add.rs",
            "use pwf_application::pending_work::add;\n",
        )];

        assert!(check_infra_containment(&files).is_empty());
    }

    #[test]
    fn load_source_tree_finds_rs_files_recursively_with_slash_relative_paths() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("crates/cli/src/engines");
        fs::create_dir_all(&nested).unwrap();
        fs::write(dir.path().join("crates/cli/src/main.rs"), "fn main() {}").unwrap();
        fs::write(nested.join("mod.rs"), "pub mod pending_work;").unwrap();
        fs::write(dir.path().join("crates/cli/src/README.md"), "not rust").unwrap();

        let mut files = load_source_tree(dir.path(), "crates/cli/src").unwrap();
        files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));

        let paths: Vec<&str> = files.iter().map(|f| f.relative_path.as_str()).collect();
        assert_eq!(
            paths,
            ["crates/cli/src/engines/mod.rs", "crates/cli/src/main.rs"]
        );
    }

    #[test]
    fn load_source_tree_propagates_a_missing_root() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_source_tree(dir.path(), "does/not/exist").is_err());
    }

    #[test]
    fn flags_a_rendering_macro_in_ordinary_application_code() {
        let files = [source_file(
            "crates/application/src/pending_work/whatever.rs",
            "pub fn summarize(id: &str) -> String {\n    format!(\"id: {id}\")\n}\n",
        )];

        let violations = check_application_rendering(&files);

        assert_eq!(violations.len(), 1);
        assert_eq!(
            violations[0].relative_path,
            "crates/application/src/pending_work/whatever.rs"
        );
        assert_eq!(violations[0].line, 2);
    }

    #[test]
    fn does_not_flag_a_cfg_test_mod_body() {
        let files = [source_file(
            "crates/application/src/pending_work/whatever.rs",
            "pub fn real() -> u32 {\n    1\n}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {\n        let s = format!(\"{}\", 1);\n        assert_eq!(s, \"1\");\n    }\n}\n",
        )];

        assert!(check_application_rendering(&files).is_empty());
    }

    #[test]
    fn does_not_flag_a_single_line_error_attribute() {
        let files = [source_file(
            "crates/application/src/pending_work/whatever.rs",
            "#[derive(Debug, thiserror::Error)]\npub enum E {\n    #[error(\"Unknown ids: {}.\", ids.join(\", \"))]\n    Bad { ids: Vec<String> },\n}\n",
        )];

        assert!(check_application_rendering(&files).is_empty());
    }

    #[test]
    fn does_not_flag_a_multi_line_error_attribute() {
        let files = [source_file(
            "crates/application/src/pending_work/whatever.rs",
            "#[derive(Debug, thiserror::Error)]\npub enum E {\n    #[error(\n        \"nothing to update (pass --prompt, --title).\"\n    )]\n    Empty,\n}\n",
        )];

        assert!(check_application_rendering(&files).is_empty());
    }

    #[test]
    fn does_not_flag_an_allowlisted_function() {
        let files = [source_file(
            "crates/application/src/pending_work/done.rs",
            "pub(super) fn review_task_prompt(reviewed_id: &str) -> String {\n    format!(\"review {reviewed_id}\")\n}\n",
        )];

        assert!(check_application_rendering(&files).is_empty());
    }

    #[test]
    fn does_not_flag_an_externally_cfg_test_gated_module_file() {
        let files = [
            source_file(
                "crates/application/src/lib.rs",
                "pub mod pending_work;\n\n#[cfg(test)]\nmod testing;\n",
            ),
            source_file(
                "crates/application/src/testing.rs",
                "pub fn locator(id: &str) -> String {\n    format!(\"/mem/{id}.md\")\n}\n",
            ),
        ];

        assert!(check_application_rendering(&files).is_empty());
    }

    #[test]
    fn a_cfg_test_marker_before_a_non_mod_item_does_not_swallow_later_real_code() {
        // Mirrors `crates/cli/src/engines/pending_work/mod.rs`'s `#[cfg(test)] pub(crate) use
        // run::store_for;` shape: the marker precedes a `use`, not a `mod` body, so it must not
        // gobble everything up to the file's real, unrelated `mod tests {` block further down.
        let files = [source_file(
            "crates/application/src/pending_work/whatever.rs",
            "#[cfg(test)]\npub(crate) use something::helper;\n\npub fn real() -> String {\n    format!(\"real\")\n}\n\n#[cfg(test)]\nmod tests {\n    fn t() {}\n}\n",
        )];

        let violations = check_application_rendering(&files);

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].line, 5);
    }

    #[test]
    fn flags_each_rendering_macro_kind() {
        let files = [source_file(
            "crates/application/src/pending_work/whatever.rs",
            "fn a() { let _ = format!(\"x\"); }\nfn b() { let _ = write!(std::io::sink(), \"x\"); }\nfn c() { let _ = writeln!(std::io::sink(), \"x\"); }\nfn d() { println!(\"x\"); }\nfn e() { eprintln!(\"x\"); }\n",
        )];

        assert_eq!(check_application_rendering(&files).len(), 5);
    }

    #[test]
    fn flags_an_aliased_execute_import() {
        let files = [source_file(
            "crates/cli/src/engines/pending_work/actions/add.rs",
            "use pwf_application::pending_work::add::execute;\n",
        )];

        let violations = check_direct_call_shape(&files);

        assert_eq!(violations.len(), 1);
        assert_eq!(
            violations[0].relative_path,
            "crates/cli/src/engines/pending_work/actions/add.rs"
        );
        assert_eq!(violations[0].line, 1);
    }

    #[test]
    fn does_not_flag_a_fully_qualified_call_with_type_only_imports() {
        let files = [source_file(
            "crates/cli/src/engines/pending_work/actions/add.rs",
            "use pwf_application::pending_work::add::AddPendingWorkError;\n\nfn run() {\n    let result = pwf_application::pending_work::add::execute(command, &store);\n}\n",
        )];

        assert!(check_direct_call_shape(&files).is_empty());
    }

    #[test]
    fn flags_an_as_renamed_operation_module() {
        let files = [source_file(
            "crates/cli/src/engines/pending_work/actions/add.rs",
            "use pwf_application::pending_work::add as add_op;\n",
        )];

        let violations = check_direct_call_shape(&files);

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].line, 1);
    }

    #[test]
    fn does_not_flag_a_renamed_type_import() {
        let files = [source_file(
            "crates/cli/src/engines/pending_work/actions/add.rs",
            "use pwf_application::pending_work::add::AddPendingWorkError as AddErr;\n",
        )];

        assert!(check_direct_call_shape(&files).is_empty());
    }

    #[test]
    fn ignores_files_outside_engines() {
        let files = [source_file(
            "crates/cli/src/config.rs",
            "use pwf_application::pending_work::add::execute;\n",
        )];

        assert!(check_direct_call_shape(&files).is_empty());
    }

    #[test]
    fn ignores_a_braced_import_group_with_no_execute_or_alias() {
        let files = [source_file(
            "crates/cli/src/engines/pending_work/query.rs",
            "use pwf_application::{\n    AppDbStore, PendingWorkItem,\n    pending_work::find::{FindPendingWork, FindPendingWorkError},\n};\n",
        )];

        assert!(check_direct_call_shape(&files).is_empty());
    }

    #[test]
    fn flags_a_bare_operation_module_import() {
        // The smuggle shape: no `execute` in the import, no `as` — but the shortened
        // `add::execute(...)` call it enables defeats the fully-qualified invariant.
        let files = [source_file(
            "crates/cli/src/engines/pending_work/actions/add.rs",
            "use pwf_application::pending_work::add;\n\nfn run() {\n    let result = add::execute(command, &store);\n}\n",
        )];

        let violations = check_direct_call_shape(&files);

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].line, 1);
        assert!(
            violations[0].message.contains("`add`"),
            "{}",
            violations[0].message
        );
    }

    #[test]
    fn flags_only_the_lowercase_segment_in_a_mixed_import_group() {
        let files = [source_file(
            "crates/cli/src/engines/pending_work/actions/add.rs",
            "use pwf_application::pending_work::{add, AddPendingWorkItem};\n",
        )];

        let violations = check_direct_call_shape(&files);

        assert_eq!(violations.len(), 1);
        assert!(
            violations[0].message.contains("`add`"),
            "{}",
            violations[0].message
        );
        assert!(!violations[0].message.contains("AddPendingWorkItem"));
    }

    #[test]
    fn flags_a_self_import_in_a_nested_group() {
        let files = [source_file(
            "crates/cli/src/engines/pending_work/actions/add.rs",
            "use pwf_application::pending_work::add::{self, AddPendingWorkError};\n",
        )];

        assert_eq!(check_direct_call_shape(&files).len(), 1);
    }

    #[test]
    fn flags_a_glob_import() {
        let files = [source_file(
            "crates/cli/src/engines/pending_work/actions/add.rs",
            "use pwf_application::pending_work::add::*;\n",
        )];

        assert_eq!(check_direct_call_shape(&files).len(), 1);
    }
}
