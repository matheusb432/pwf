//! Fixture-corpus integration tests for the `rename-project` engine.
//!
//! Each test stages a self-contained pwf-db in a `TempDir`: a project dir with a
//! conventional and descriptive filenames (one carrying an internal
//! `[[OLD-NNNN]]` cross-ref), a second project whose frontmatter
//! lists a cross-project `prereq: "[[OLD-0001]]"`, and a `.trash/` note that must
//! be left untouched — plus a pwf config JSON and a `repos.toml` manifest.

use std::{
    fs,
    path::{Path, PathBuf},
};

use pwf::{cli, command, engines::rename_project};

struct Vault {
    _dir: tempfile::TempDir,
    root: PathBuf,
    cfg: PathBuf,
    manifest: PathBuf,
}

fn note(id: &str, status: &str, project: &str, body: &str) -> String {
    format!(
        "---\nid: {id}\nstatus: {status}\ntitle: t\nproject: {project}\ncreated: 2026-01-01\n---\n\n{body}\n"
    )
}

fn stage() -> Vault {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();

    let old = root.join("self/oldproj");
    fs::create_dir_all(&old).unwrap();
    fs::write(
        old.join("OLD-0001.md"),
        note("OLD-0001", "active", "oldproj", "see [[OLD-0002]]"),
    )
    .unwrap();
    fs::write(
        old.join("descriptive-task.md"),
        note("OLD-0002", "active", "oldproj", "body"),
    )
    .unwrap();
    fs::write(
        old.join("completed-task.md"),
        note("OLD-0007", "done", "oldproj", "completed"),
    )
    .unwrap();

    // The project index file is named by the path basename and links its items.
    fs::write(
        old.join("oldproj.md"),
        "---\nid: old\ntitle: oldproj\n---\n\n# oldproj\n\n- [ ] [[OLD-0001]]\n- [ ] [[OLD-0002]]\n",
    )
    .unwrap();

    let other = root.join("self/other");
    fs::create_dir_all(&other).unwrap();
    fs::write(
        other.join("OTH-0005.md"),
        "---\nid: OTH-0005\nstatus: active\ntitle: t\nproject: other\nprereq: \"[[OLD-0001]]\"\ncreated: 2026-01-01\n---\n\nx\n",
    )
    .unwrap();

    let trash = old.join(".trash");
    fs::create_dir_all(&trash).unwrap();
    fs::write(
        trash.join("OLD-0099.md"),
        note("OLD-0099", "done", "oldproj", "trashed [[OLD-0001]]"),
    )
    .unwrap();

    let cfg = root.join("pending-work.json");
    let notes_base = root.join("self");
    fs::write(
        &cfg,
        format!(
            r#"{{"notesDir": {}, "projects": {{}}, "prefixes": {{"oldproj": "OLD"}}}}"#,
            serde_json::to_string(notes_base.to_str().unwrap()).unwrap()
        ),
    )
    .unwrap();

    let manifest = root.join("repos.toml");
    fs::write(
        &manifest,
        "[[repo]]\npath = \"self/oldproj\"\nremote = \"git@x:me/oldproj.git\"\ncode = \"OLD\"\n\n[[repo]]\npath = \"self/other\"\ncode = \"OTH\"\n",
    )
    .unwrap();

    Vault {
        _dir: dir,
        root,
        cfg,
        manifest,
    }
}

fn args(v: &Vault, extra: &[&str]) -> cli::Args {
    let mut argv = vec![
        "rename-project".to_string(),
        "--config-path".into(),
        v.cfg.to_str().unwrap().into(),
        "--manifest-path".into(),
        v.manifest.to_str().unwrap().into(),
    ];
    argv.extend(extra.iter().map(|s| (*s).to_string()));
    match command::parse_command_argv(argv).expect("parse") {
        command::ParsedCommand::RenameProject(a) => a,
        other => panic!("expected rename-project command, got {other:?}"),
    }
}

fn snapshot(root: &Path) -> Vec<(String, String)> {
    let mut v: Vec<_> = walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .map(|e| {
            (
                e.path()
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
                fs::read_to_string(e.path()).unwrap_or_default(),
            )
        })
        .collect();
    v.sort();
    v
}

fn read(root: &Path, rel: &str) -> String {
    fs::read_to_string(root.join(rel)).unwrap()
}

#[test]
fn dry_run_is_a_no_op_and_lists_plan() {
    let v = stage();
    let before = snapshot(&v.root);
    let manifest_before = fs::read_to_string(&v.manifest).unwrap();

    let out = rename_project::run(&args(
        &v,
        &[
            "--old",
            "OLD",
            "--new",
            "NEW",
            "--new-path",
            "self/newproj",
            "--dry-run",
        ],
    ))
    .unwrap();

    assert_eq!(snapshot(&v.root), before, "dry-run mutated the tree");
    assert_eq!(
        fs::read_to_string(&v.manifest).unwrap(),
        manifest_before,
        "dry-run mutated the manifest"
    );
    assert!(
        out.contains("self/oldproj") && out.contains("self/newproj"),
        "dir move listed:\n{out}"
    );
    assert!(
        out.contains("OLD-0001.md") && out.contains("NEW-0001.md"),
        "file rename listed:\n{out}"
    );
    assert!(
        !out.contains("descriptive-task.md ->"),
        "descriptive filename remains a locator:\n{out}"
    );
    assert!(
        out.contains("OTH-0005.md"),
        "cross-project token rewrite listed:\n{out}"
    );
    assert!(out.contains("project:"), "label update listed:\n{out}");
    assert!(
        out.contains("oldproj.md -> newproj.md"),
        "index file rename listed:\n{out}"
    );
    assert!(
        out.to_lowercase().contains("repos.toml") || out.contains("code ="),
        "manifest edit listed:\n{out}"
    );
    assert!(!out.contains("OLD-0099"), ".trash must be excluded:\n{out}");
}

#[test]
fn rename_with_new_path_moves_renames_relinks_and_updates_label() {
    let v = stage();
    let out = rename_project::run(&args(
        &v,
        &["--old", "OLD", "--new", "NEW", "--new-path", "self/newproj"],
    ))
    .unwrap();
    assert!(out.contains("done"), "summary: {out}");

    assert!(!v.root.join("self/oldproj").exists(), "old dir gone");
    assert!(
        v.root.join("self/newproj/NEW-0001.md").exists(),
        "renamed active note"
    );
    assert!(
        v.root.join("self/newproj/descriptive-task.md").exists(),
        "descriptive task filename preserved"
    );
    assert!(
        !v.root.join("self/newproj/OLD-0001.md").exists(),
        "old id gone"
    );

    assert!(
        read(&v.root, "self/newproj/NEW-0001.md").contains("[[NEW-0002]]"),
        "internal cross-ref rewritten"
    );
    assert!(
        read(&v.root, "self/newproj/descriptive-task.md").contains("id: NEW-0002"),
        "descriptive note identity rewritten in frontmatter"
    );
    assert!(
        read(&v.root, "self/newproj/NEW-0001.md").contains("project: newproj"),
        "project: label updated to new basename"
    );

    let oth = read(&v.root, "self/other/OTH-0005.md");
    assert!(
        oth.contains("[[NEW-0001]]"),
        "cross-project prereq relinked"
    );
    assert!(
        oth.contains("project: other"),
        "other project's label untouched"
    );

    assert!(
        read(&v.root, "self/newproj/.trash/OLD-0099.md").contains("[[OLD-0001]]"),
        ".trash content must be left as-is"
    );

    assert!(
        v.root.join("self/newproj/newproj.md").exists(),
        "index renamed to the new basename"
    );
    assert!(
        !v.root.join("self/newproj/oldproj.md").exists(),
        "old index name gone"
    );
    assert!(
        read(&v.root, "self/newproj/newproj.md").contains("[[NEW-0001]]"),
        "index links rewritten by the vault token pass"
    );
    assert!(
        read(&v.root, "self/newproj/newproj.md").starts_with("---\nid: new\ntitle: newproj\n---"),
        "project-index identity rewritten"
    );

    let man = fs::read_to_string(&v.manifest).unwrap();
    assert!(
        man.contains("path = \"self/newproj\""),
        "manifest path updated"
    );
    assert!(man.contains("code = \"NEW\""), "manifest code updated");
    assert!(man.contains("code = \"OTH\""), "sibling entry preserved");
    assert!(
        man.contains("remote = \"git@x:me/oldproj.git\""),
        "remote preserved"
    );
}

#[test]
fn code_only_rename_keeps_dir_and_label() {
    let v = stage();
    rename_project::run(&args(&v, &["--old", "OLD", "--new", "NEW"])).unwrap();

    let dir = v.root.join("self/oldproj");
    assert!(dir.join("NEW-0001.md").exists(), "renamed in place");
    assert!(!dir.join("OLD-0001.md").exists(), "old id gone");
    assert!(
        dir.join("completed-task.md").exists(),
        "descriptive completed note preserved"
    );

    assert!(
        read(&v.root, "self/oldproj/NEW-0001.md").contains("project: oldproj"),
        "project: label unchanged on code-only rename"
    );
    assert!(
        read(&v.root, "self/oldproj/NEW-0001.md").contains("[[NEW-0002]]"),
        "internal cross-ref still rewritten"
    );
    assert!(
        read(&v.root, "self/other/OTH-0005.md").contains("[[NEW-0001]]"),
        "cross-project prereq relinked"
    );

    assert!(
        dir.join("oldproj.md").exists(),
        "index file NOT renamed on a code-only rename"
    );
    assert!(!dir.join("newproj.md").exists());
    assert!(
        read(&v.root, "self/oldproj/oldproj.md").contains("[[NEW-0001]]"),
        "index links still rewritten by the vault token pass"
    );
    assert!(read(&v.root, "self/oldproj/oldproj.md").contains("id: new"));

    let man = fs::read_to_string(&v.manifest).unwrap();
    assert!(man.contains("path = \"self/oldproj\""), "path preserved");
    assert!(man.contains("code = \"NEW\""), "code updated");
}

#[test]
fn index_file_renamed_only_when_basename_changes() {
    // Path change with a new basename → index renamed and re-linked.
    let v = stage();
    rename_project::run(&args(
        &v,
        &["--old", "OLD", "--new", "NEW", "--new-path", "self/newproj"],
    ))
    .unwrap();
    assert!(v.root.join("self/newproj/newproj.md").exists());
    assert!(!v.root.join("self/newproj/oldproj.md").exists());

    // A path change whose basename is unchanged keeps the index name.
    let v2 = stage();
    rename_project::run(&args(
        &v2,
        &[
            "--old",
            "OLD",
            "--new",
            "NEW",
            "--new-path",
            "nested/oldproj",
        ],
    ))
    .unwrap();
    assert!(
        v2.root.join("nested/oldproj/oldproj.md").exists(),
        "same basename => index keeps its name even though the dir moved"
    );
}

#[test]
fn collision_aborts_with_zero_writes() {
    let v = stage();
    fs::write(
        v.root.join("self/oldproj/NEW-0001.md"),
        "---\ntype: note\n---\n\ncollide\n",
    )
    .unwrap();
    let before = snapshot(&v.root);
    let manifest_before = fs::read_to_string(&v.manifest).unwrap();

    let err = rename_project::run(&args(&v, &["--old", "OLD", "--new", "NEW"])).unwrap_err();
    assert!(err.to_lowercase().contains("collision"), "got: {err}");
    assert_eq!(snapshot(&v.root), before, "no partial writes");
    assert_eq!(fs::read_to_string(&v.manifest).unwrap(), manifest_before);
}

#[test]
fn unknown_old_code_errors_without_writes() {
    let v = stage();
    let before = snapshot(&v.root);
    let err = rename_project::run(&args(&v, &["--old", "ZZZ", "--new", "NEW"])).unwrap_err();
    assert!(
        err.to_lowercase().contains("not registered") || err.to_lowercase().contains("zzz"),
        "got: {err}"
    );
    assert_eq!(snapshot(&v.root), before);
}

#[test]
fn old_equals_new_without_path_is_noop_error() {
    let v = stage();
    let err = rename_project::run(&args(&v, &["--old", "OLD", "--new", "OLD"])).unwrap_err();
    assert!(err.to_lowercase().contains("nothing to do"), "got: {err}");
}

#[test]
fn path_only_move_with_same_code_is_allowed() {
    let v = stage();
    rename_project::run(&args(
        &v,
        &["--old", "OLD", "--new", "OLD", "--new-path", "self/moved"],
    ))
    .unwrap();
    assert!(
        v.root.join("self/moved/OLD-0001.md").exists(),
        "moved, same id"
    );
    assert!(!v.root.join("self/oldproj").exists(), "old dir gone");
    assert!(
        read(&v.root, "self/moved/OLD-0001.md").contains("project: moved"),
        "label follows the new basename even with an unchanged code"
    );
}
