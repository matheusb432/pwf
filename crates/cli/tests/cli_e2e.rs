//! Checks the built binary's arguments, output, exit codes, and persisted effects.

use std::fs;

use assert_cmd::Command;
use predicates::{prelude::PredicateBooleanExt, str::contains};
use tempfile::TempDir;

const LEADING_HYPHEN_TAG: &str = "-sqlite";

fn pwf() -> Command {
    Command::cargo_bin("pwf").unwrap()
}

fn finish_fixture(dir: TempDir, cfg: std::path::PathBuf) -> (TempDir, std::path::PathBuf) {
    migrate_fixture(&cfg);
    (dir, cfg)
}

fn migrate_fixture(cfg: &std::path::Path) {
    let config: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(cfg).unwrap()).unwrap();
    let notes_dir = std::path::PathBuf::from(config["notesDir"].as_str().unwrap());
    let projects = config["projects"].as_object().unwrap();
    let prefixes = config["prefixes"].as_object().unwrap();
    for project in projects.keys() {
        let prefix = prefixes[project].as_str().unwrap();
        let project_dir = notes_dir.join(project);
        let index_path = project_dir.join(format!("{project}.md"));
        if let Ok(content) = fs::read_to_string(&index_path)
            && !content.starts_with("---")
        {
            fs::write(
                &index_path,
                format!(
                    "---\nid: {}\ntitle: {project}\n---\n\n{content}",
                    prefix.to_ascii_lowercase()
                ),
            )
            .unwrap();
        }
        let Ok(entries) = fs::read_dir(project_dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path == index_path || path.extension().and_then(|ext| ext.to_str()) != Some("md") {
                continue;
            }
            let Some(id) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            if !id.starts_with(&format!("{prefix}-")) {
                continue;
            }
            let Ok(content) = fs::read_to_string(&path) else {
                continue;
            };
            if content.lines().any(|line| line.starts_with("id:"))
                || content.lines().any(|line| line == "type: note")
            {
                continue;
            }
            if let Some(rest) = content.strip_prefix("---\n") {
                fs::write(&path, format!("---\nid: {id}\n{rest}")).unwrap();
            }
        }
    }
}

/// Stages one open item and its config.
fn staged() -> (TempDir, std::path::PathBuf) {
    let dir = TempDir::new().unwrap();
    let notes = dir.path().join("notes");
    let proj = notes.join("glep-shimeji");
    fs::create_dir_all(&proj).unwrap();
    fs::write(
        proj.join("GLP-0001.md"),
        "---\nid: GLP-0001\nstatus: active\ntitle: tray gui\nproject: glep-shimeji\ncreated: 2026-01-01\n---\n\nadd toggle\n",
    )
    .unwrap();
    fs::write(
        proj.join("glep-shimeji.md"),
        "- [ ] [[GLP-0001|tray gui]]\n",
    )
    .unwrap();
    let cfg = dir.path().join("cfg.json");
    fs::write(
        &cfg,
        format!(
            r#"{{ "notesDir": {:?}, "projects": {{ "glep-shimeji": "/repo" }}, "prefixes": {{ "glep-shimeji": "GLP" }} }}"#,
            notes.to_string_lossy()
        ),
    )
    .unwrap();
    finish_fixture(dir, cfg)
}

fn staged_tagged_items() -> (TempDir, std::path::PathBuf) {
    let dir = TempDir::new().unwrap();
    let notes = dir.path().join("notes");
    let project = notes.join("glep-shimeji");
    fs::create_dir_all(&project).unwrap();
    for (id, title, tags) in [
        ("GLP-0001", "both tags", Some("[sqlite, godot]")),
        ("GLP-0002", "sqlite only", Some("[sqlite]")),
        ("GLP-0003", "untagged", None),
    ] {
        let tags = tags.map_or_else(String::new, |value| format!("tags: {value}\n"));
        fs::write(
            project.join(format!("{id}.md")),
            format!(
                "---\nstatus: active\ntitle: {title}\nproject: glep-shimeji\ncreated: 2026-01-01\n{tags}---\n\nbody\n"
            ),
        )
        .unwrap();
    }
    fs::write(
        project.join("glep-shimeji.md"),
        "- [ ] [[GLP-0001|both tags]]\n- [ ] [[GLP-0002|sqlite only]]\n- [ ] [[GLP-0003|untagged]]\n",
    )
    .unwrap();
    let cfg = dir.path().join("cfg.json");
    fs::write(
        &cfg,
        format!(
            r#"{{ "notesDir": {:?}, "projects": {{ "glep-shimeji": "/repo" }}, "prefixes": {{ "glep-shimeji": "GLP" }} }}"#,
            notes.to_string_lossy()
        ),
    )
    .unwrap();
    finish_fixture(dir, cfg)
}

/// Stages a second item for prerequisite tests without changing single-item fixtures.
fn staged_two() -> (TempDir, std::path::PathBuf) {
    let (dir, cfg) = staged();
    let proj = dir.path().join("notes").join("glep-shimeji");
    fs::write(
        proj.join("GLP-0002.md"),
        "---\nstatus: active\ntitle: second\nproject: glep-shimeji\ncreated: 2026-01-02\n---\n\ndo more\n",
    )
    .unwrap();
    fs::write(
        proj.join("glep-shimeji.md"),
        "- [ ] [[GLP-0002|second]]\n- [ ] [[GLP-0001|tray gui]]\n",
    )
    .unwrap();
    finish_fixture(dir, cfg)
}

/// Stages two items whose creation and ID orders disagree.
fn staged_two_diverging_created() -> (TempDir, std::path::PathBuf) {
    let dir = TempDir::new().unwrap();
    let notes = dir.path().join("notes");
    let proj = notes.join("glep-shimeji");
    fs::create_dir_all(&proj).unwrap();
    fs::write(
        proj.join("GLP-0001.md"),
        "---\nstatus: active\ntitle: tray gui\nproject: glep-shimeji\ncreated: 2026-03-01\n---\n\nadd toggle\n",
    )
    .unwrap();
    fs::write(
        proj.join("GLP-0002.md"),
        "---\nstatus: active\ntitle: second\nproject: glep-shimeji\ncreated: 2026-01-01\n---\n\ndo more\n",
    )
    .unwrap();
    fs::write(
        proj.join("glep-shimeji.md"),
        "- [ ] [[GLP-0001|tray gui]]\n- [ ] [[GLP-0002|second]]\n",
    )
    .unwrap();
    let cfg = dir.path().join("cfg.json");
    fs::write(
        &cfg,
        format!(
            r#"{{ "notesDir": {:?}, "projects": {{ "glep-shimeji": "/repo" }}, "prefixes": {{ "glep-shimeji": "GLP" }} }}"#,
            notes.to_string_lossy()
        ),
    )
    .unwrap();
    finish_fixture(dir, cfg)
}

/// Stages two projects whose project-name and creation-date orders disagree.
fn staged_two_projects_diverging_created() -> (TempDir, std::path::PathBuf) {
    let dir = TempDir::new().unwrap();
    let notes = dir.path().join("notes");
    let cfg_proj = notes.join("config-handler");
    let glp_proj = notes.join("glep-shimeji");
    fs::create_dir_all(&cfg_proj).unwrap();
    fs::create_dir_all(&glp_proj).unwrap();
    fs::write(
        cfg_proj.join("CFG-0001.md"),
        "---\nstatus: active\ntitle: cfg item\nproject: config-handler\ncreated: 2026-01-01\n---\n\ndo cfg\n",
    )
    .unwrap();
    fs::write(
        cfg_proj.join("config-handler.md"),
        "- [ ] [[CFG-0001|cfg item]]\n",
    )
    .unwrap();
    fs::write(
        glp_proj.join("GLP-0099.md"),
        "---\nstatus: active\ntitle: glp item\nproject: glep-shimeji\ncreated: 2026-03-01\n---\n\ndo glp\n",
    )
    .unwrap();
    fs::write(
        glp_proj.join("glep-shimeji.md"),
        "- [ ] [[GLP-0099|glp item]]\n",
    )
    .unwrap();
    let cfg = dir.path().join("cfg.json");
    fs::write(
        &cfg,
        format!(
            r#"{{ "notesDir": {:?}, "projects": {{ "config-handler": "/repo/cfg", "glep-shimeji": "/repo/glp" }}, "prefixes": {{ "config-handler": "CFG", "glep-shimeji": "GLP" }} }}"#,
            notes.to_string_lossy()
        ),
    )
    .unwrap();
    finish_fixture(dir, cfg)
}

/// Stages mixed lifecycle records across two managed projects.
fn status_fixture_stage() -> (TempDir, std::path::PathBuf) {
    let dir = TempDir::new().unwrap();
    let notes = dir.path().join("notes");
    let project_glp = notes.join("glep-shimeji");
    let project_cfg = notes.join("config-handler");
    let repo_glp = dir.path().join("repo-glp");
    let repo_cfg = dir.path().join("repo-cfg");
    fs::create_dir_all(&project_glp).unwrap();
    fs::create_dir_all(&project_cfg).unwrap();
    fs::create_dir_all(&repo_glp).unwrap();
    fs::create_dir_all(&repo_cfg).unwrap();

    for (directory, id, status, title, project, created) in [
        (
            &project_glp,
            "GLP-0001",
            "active",
            "active default",
            "glep-shimeji",
            "2026-07-01",
        ),
        (
            &project_glp,
            "GLP-0002",
            "active",
            "active human",
            "glep-shimeji",
            "2026-07-02",
        ),
        (
            &project_glp,
            "GLP-0003",
            "done",
            "done human linked",
            "glep-shimeji",
            "2026-07-03",
        ),
        (
            &project_glp,
            "GLP-0004",
            "done",
            "done unlinked",
            "glep-shimeji",
            "2026-07-04",
        ),
        (
            &project_glp,
            "GLP-0005",
            "cancelled",
            "cancelled unlinked",
            "glep-shimeji",
            "2026-07-05",
        ),
        (
            &project_glp,
            "GLP-0006",
            "active",
            "active orphan",
            "glep-shimeji",
            "2026-07-06",
        ),
        (
            &project_cfg,
            "CFG-0001",
            "done",
            "cfg done unlinked",
            "config-handler",
            "2026-07-07",
        ),
        (
            &project_cfg,
            "CFG-0002",
            "active",
            "cfg active default",
            "config-handler",
            "2026-07-08",
        ),
        (
            &project_cfg,
            "CFG-0003",
            "cancelled",
            "cfg cancelled unlinked",
            "config-handler",
            "2026-07-09",
        ),
    ] {
        fs::write(
            directory.join(format!("{id}.md")),
            format!(
                "---\nid: {id}\nstatus: {status}\ntitle: {title}\nproject: {project}\ncreated: {created}\n---\n\nrun {title}\n"
            ),
        )
        .unwrap();
    }

    fs::write(
        project_glp.join("glep-shimeji.md"),
        "- [ ] [[GLP-0001|active default]]\n\n## Human\n\n- [ ] [[GLP-0002|active human]]\n- [x] [[GLP-0003|done human linked]] ✅ 2026-07-03\n",
    )
    .unwrap();
    fs::write(
        project_cfg.join("config-handler.md"),
        "- [ ] [[CFG-0002|cfg active default]]\n",
    )
    .unwrap();

    let cfg = dir.path().join("cfg.json");
    fs::write(
        &cfg,
        format!(
            r#"{{ "notesDir": {:?}, "projects": {{ "config-handler": {:?}, "glep-shimeji": {:?} }}, "prefixes": {{ "config-handler": "CFG", "glep-shimeji": "GLP" }} }}"#,
            notes.to_string_lossy(),
            repo_cfg.to_string_lossy(),
            repo_glp.to_string_lossy()
        ),
    )
    .unwrap();
    finish_fixture(dir, cfg)
}

fn status_command_output(cfg: &std::path::Path, args: &[&str]) -> std::process::Output {
    let output = pwf()
        .args(args)
        .arg("--config-path")
        .arg(cfg)
        .env_remove("CLICOLOR_FORCE")
        .env_remove("NO_COLOR")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

/// Stages a real repository with one handoff for `--continue-handoff`.
fn staged_with_handoff() -> (TempDir, std::path::PathBuf) {
    let dir = TempDir::new().unwrap();
    let notes = dir.path().join("notes");
    let proj = notes.join("glep-shimeji");
    let repo = dir.path().join("repo");
    let handoffs = repo.join("docs").join("handoffs");
    fs::create_dir_all(&proj).unwrap();
    fs::create_dir_all(&handoffs).unwrap();
    fs::write(proj.join("glep-shimeji.md"), "# glep-shimeji\n").unwrap();
    fs::write(
        handoffs.join("2026-01-01-api-cleanup.md"),
        "# API cleanup handoff\n",
    )
    .unwrap();
    let cfg = dir.path().join("cfg.json");
    fs::write(
        &cfg,
        format!(
            r#"{{ "notesDir": {:?}, "projects": {{ "glep-shimeji": {:?} }}, "prefixes": {{ "glep-shimeji": "GLP" }} }}"#,
            notes.to_string_lossy(),
            repo.to_string_lossy()
        ),
    )
    .unwrap();
    finish_fixture(dir, cfg)
}

/// Stages a fresh repository without Git metadata for the handoff lifecycle round trip.
fn staged_for_handoff_mirror_roundtrip() -> (TempDir, std::path::PathBuf) {
    let dir = TempDir::new().unwrap();
    let notes = dir.path().join("notes");
    let proj = notes.join("glep-shimeji");
    let repo = dir.path().join("repo");
    fs::create_dir_all(&proj).unwrap();
    fs::create_dir_all(repo.join("docs").join("handoffs")).unwrap();
    fs::write(proj.join("glep-shimeji.md"), "# glep-shimeji\n").unwrap();
    let cfg = dir.path().join("cfg.json");
    fs::write(
        &cfg,
        format!(
            r#"{{ "notesDir": {:?}, "projects": {{ "glep-shimeji": {:?} }}, "prefixes": {{ "glep-shimeji": "GLP" }} }}"#,
            notes.to_string_lossy(),
            repo.to_string_lossy()
        ),
    )
    .unwrap();
    finish_fixture(dir, cfg)
}

fn read_index(dir: &TempDir) -> String {
    fs::read_to_string(dir.path().join("notes/glep-shimeji/glep-shimeji.md")).unwrap()
}

/// Stages `count` open items for list-cap tests.
fn staged_many(count: usize) -> (TempDir, std::path::PathBuf) {
    let dir = TempDir::new().unwrap();
    let notes = dir.path().join("notes");
    let proj = notes.join("glep-shimeji");
    fs::create_dir_all(&proj).unwrap();
    let mut index = String::from("# glep-shimeji\n\n");
    for n in 1..=count {
        let id = format!("GLP-{n:04}");
        fs::write(
            proj.join(format!("{id}.md")),
            format!("---\nstatus: active\ntitle: t{n}\nproject: glep-shimeji\ncreated: 2026-01-01\n---\n\nbody\n"),
        )
        .unwrap();
        index.push_str(&format!("- [ ] [[{id}|t{n}]]\n"));
    }
    fs::write(proj.join("glep-shimeji.md"), index).unwrap();
    let cfg = dir.path().join("cfg.json");
    fs::write(
        &cfg,
        format!(
            r#"{{ "notesDir": {:?}, "projects": {{ "glep-shimeji": "/repo" }}, "prefixes": {{ "glep-shimeji": "GLP" }} }}"#,
            notes.to_string_lossy()
        ),
    )
    .unwrap();
    finish_fixture(dir, cfg)
}

#[test]
fn add_positional_quoted_prompt_creates_item() {
    let (d, cfg) = staged();
    pwf()
        .args(["add", "glep-shimeji", "x y z", "--config-path"])
        .arg(&cfg)
        .assert()
        .success();
    assert!(d.path().join("notes/glep-shimeji/GLP-0002.md").exists());
    assert!(read_index(&d).contains("[[GLP-0002]]"), "index not updated");
}

#[test]
fn add_confirmation_leads_with_added_task_prefix_and_no_blank_line() {
    let (_d, cfg) = staged();
    let out = pwf()
        .args(["add", "glep-shimeji", "x y z", "--config-path"])
        .arg(&cfg)
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.starts_with("Added pwf task: **GLP-0002 glep-shimeji ::"),
        "got: {stdout}"
    );
    assert!(stdout.contains("file:"), "got: {stdout}");
}

#[test]
fn add_bare_words_joined_into_prompt() {
    let (d, cfg) = staged();
    pwf()
        .args(["add", "glep-shimeji", "do", "a", "thing", "--config-path"])
        .arg(&cfg)
        .assert()
        .success();
    let item = fs::read_to_string(d.path().join("notes/glep-shimeji/GLP-0002.md")).unwrap();
    assert!(item.contains("do a thing"), "prompt not joined: {item}");
}

#[test]
fn add_human_flag_files_under_human_section() {
    let (d, cfg) = staged();
    pwf()
        .args(["add", "glep-shimeji", "x", "--human", "--config-path"])
        .arg(&cfg)
        .assert()
        .success();
    let index = read_index(&d);
    let human = index.find("## Human").expect("no ## Human section");
    let item = index.find("[[GLP-0002]]").expect("no new item link");
    assert!(item > human, "item not under ## Human: {index}");
}

#[test]
fn add_human_flag_emits_section_created_diagnostic_when_it_creates_human_section() {
    let (_d, cfg) = staged();
    let output = pwf()
        .args(["add", "glep-shimeji", "x", "--human", "--config-path"])
        .arg(&cfg)
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "info: created `## Human` section in glep-shimeji\n"
    );
}

#[test]
fn add_human_flag_rejects_unreadable_index_before_mutation() {
    let dir = TempDir::new().unwrap();
    let notes = dir.path().join("notes");
    let proj = notes.join("glep-shimeji");
    fs::create_dir_all(&proj).unwrap();
    fs::write(
        proj.join("GLP-0001.md"),
        "---\nid: GLP-0001\nstatus: active\ntitle: tray gui\nproject: glep-shimeji\ncreated: 2026-01-01\n---\n\nadd toggle\n",
    )
    .unwrap();
    fs::create_dir_all(proj.join("glep-shimeji.md")).unwrap();
    let cfg = dir.path().join("cfg.json");
    fs::write(
        &cfg,
        format!(
            r#"{{ "notesDir": {:?}, "projects": {{ "glep-shimeji": "/repo" }}, "prefixes": {{ "glep-shimeji": "GLP" }} }}"#,
            notes.to_string_lossy()
        ),
    )
    .unwrap();

    let output = pwf()
        .args(["add", "glep-shimeji", "x", "--human", "--config-path"])
        .arg(&cfg)
        .output()
        .unwrap();

    assert!(!output.status.success(), "{output:?}");
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.starts_with("Error: Cannot read index: "), "{stderr}");
    assert!(!stderr.contains("created `## Human`"), "{stderr}");
}

#[test]
fn add_section_future_files_under_future_section() {
    let (d, cfg) = staged();
    pwf()
        .args([
            "add",
            "glep-shimeji",
            "x",
            "--section",
            "future",
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .success();
    let index = read_index(&d);
    let future = index.find("## Future").expect("no ## Future section");
    let item = index.find("[[GLP-0002]]").expect("no new item link");
    assert!(item > future, "item not under ## Future: {index}");
}

#[test]
fn e2e_list_scope_flags_conflict() {
    Command::cargo_bin("pwf")
        .unwrap()
        .args(["list", "--human", "--all"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("cannot be used with"));
}

#[test]
fn add_continue_handoff_builds_handoff_prompt() {
    let (d, cfg) = staged_with_handoff();
    let out = pwf()
        .args(["add", "glep-shimeji", "--continue-handoff", "--config-path"])
        .arg(&cfg)
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.starts_with("Added pwf task: **GLP-0001"),
        "id not moved to the front: {stdout}"
    );
    assert!(
        stdout.contains(":: continue api cleanup"),
        "title not in output: {stdout}"
    );
    let item = std::fs::read_to_string(d.path().join("notes/glep-shimeji/GLP-0001.md")).unwrap();
    assert!(
        item.contains("Continue the handoff at @docs/handoffs/2026-01-01-api-cleanup.md."),
        "prompt not in item: {item}"
    );
    assert!(d.path().join("notes/glep-shimeji/GLP-0001.md").exists());
}

#[test]
fn bare_words_route_errors_and_writes_nothing() {
    let (d, cfg) = staged();
    pwf()
        .args(["glep-shimeji", "make", "a", "thing", "--config-path"])
        .arg(&cfg)
        .assert()
        .failure()
        .stderr(contains("pwf add"));
    assert!(!d.path().join("notes/glep-shimeji/GLP-0002.md").exists());
}

#[test]
fn single_word_route_lists_project() {
    let (_d, cfg) = staged();
    pwf()
        .args(["glep-shimeji", "--config-path"])
        .arg(&cfg)
        .assert()
        .success()
        .stdout(contains("GLP-0001"));
}

#[test]
fn single_word_route_accepts_project_code_case_insensitively() {
    let (_d, cfg) = staged();
    pwf()
        .args(["glp", "--config-path"])
        .arg(&cfg)
        .assert()
        .success()
        .stdout(contains("GLP-0001"));
}

#[test]
fn single_word_route_rejects_project_name_prefix() {
    let (_d, cfg) = staged();
    pwf()
        .args(["glep", "--config-path"])
        .arg(&cfg)
        .assert()
        .failure()
        .stderr(contains("Unknown managed project identifier: glep"));
}

#[test]
fn help_and_version_exit_zero() {
    pwf()
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("add"))
        .stdout(contains("list"))
        .stdout(contains("handoff"));
    pwf()
        .arg("--version")
        .assert()
        .success()
        .stdout(contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn unknown_engine_fails() {
    pwf().arg("bogus").assert().failure();
}

#[test]
fn canonical_list_succeeds() {
    let (_d, cfg) = staged();
    let canon = pwf()
        .args(["list", "--config-path"])
        .arg(&cfg)
        .assert()
        .success();
    let canon_out = String::from_utf8(canon.get_output().stdout.clone()).unwrap();
    assert!(canon_out.contains("GLP-0001 :: tray gui"));
}

#[test]
fn ls_alias_matches_list_output() {
    let (_d, cfg) = staged();
    let list = pwf()
        .args(["list", "--config-path"])
        .arg(&cfg)
        .assert()
        .success();
    let ls = pwf()
        .args(["ls", "--config-path"])
        .arg(&cfg)
        .assert()
        .success();

    assert_eq!(ls.get_output().stdout, list.get_output().stdout);
}

#[test]
fn e2e_list_status_default_matches_explicit_active() {
    let (_dir, cfg) = status_fixture_stage();
    let default = status_command_output(&cfg, &["list"]);
    let active = status_command_output(&cfg, &["list", "--status", "active"]);
    let stdout = String::from_utf8(default.stdout.clone()).unwrap();

    assert_eq!(default.stdout, active.stdout);
    assert!(stdout.contains("GLP-0001 :: active default"), "{stdout}");
    assert!(
        stdout.contains("CFG-0002 :: cfg active default"),
        "{stdout}"
    );
    assert!(!stdout.contains("GLP-0002"), "{stdout}");
    assert!(!stdout.contains("GLP-0006"), "{stdout}");
    assert!(!stdout.contains("(active)"), "{stdout}");
}

#[test]
fn e2e_list_status_exact_filters_and_cancelled_alias_match() {
    let (_dir, cfg) = status_fixture_stage();
    let done = status_command_output(&cfg, &["list", "--status", "done"]);
    let done_stdout = String::from_utf8(done.stdout).unwrap();
    assert!(done_stdout.contains("CFG-0001 :: cfg done unlinked"));
    assert!(done_stdout.contains("GLP-0004 :: done unlinked"));
    assert!(!done_stdout.contains("GLP-0003"), "{done_stdout}");
    assert!(!done_stdout.contains("active default"), "{done_stdout}");
    assert!(!done_stdout.contains("cancelled unlinked"), "{done_stdout}");

    let list = status_command_output(&cfg, &["list", "--status", "cancelled"]);
    let alias = status_command_output(&cfg, &["ls", "--status", "cancelled"]);
    let cancelled_stdout = String::from_utf8(list.stdout.clone()).unwrap();
    assert_eq!(list.stdout, alias.stdout);
    assert!(cancelled_stdout.contains("CFG-0003 :: cfg cancelled unlinked"));
    assert!(cancelled_stdout.contains("GLP-0005 :: cancelled unlinked"));
    assert!(!cancelled_stdout.contains("done unlinked"));
}

#[test]
fn e2e_list_status_all_annotates_every_lifecycle_and_hides_active_orphan() {
    let (_dir, cfg) = status_fixture_stage();
    let output = status_command_output(&cfg, &["list", "--status", "all", "-n", "0"]);
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(stdout.contains("GLP-0001 :: active default (active)"));
    assert!(stdout.contains("GLP-0004 :: done unlinked (done)"));
    assert!(stdout.contains("GLP-0005 :: cancelled unlinked (cancelled)"));
    assert!(!stdout.contains("GLP-0006"), "{stdout}");
    assert!(!stdout.contains('\u{1b}'), "{stdout}");
}

#[test]
fn e2e_list_status_project_routes_keep_project_scope() {
    let (_dir, cfg) = status_fixture_stage();
    let done = status_command_output(&cfg, &["glep-shimeji", "--status", "done"]);
    let done_stdout = String::from_utf8(done.stdout).unwrap();
    assert!(done_stdout.contains("GLP-0004 :: done unlinked"));
    assert!(!done_stdout.contains("GLP-0003"), "{done_stdout}");
    assert!(!done_stdout.contains("CFG-"), "{done_stdout}");

    let shorthand = status_command_output(&cfg, &["glep-shimeji", "--status", "all"]);
    let canonical = status_command_output(
        &cfg,
        &["list", "--project", "glep-shimeji", "--status", "all"],
    );
    assert_eq!(shorthand.stdout, canonical.stdout);
}

#[test]
fn e2e_list_status_all_composes_with_all_sections() {
    let (_dir, cfg) = status_fixture_stage();
    let output = status_command_output(&cfg, &["list", "--status", "all", "--all", "-n", "0"]);
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(stdout.contains("Human\n"), "{stdout}");
    assert!(stdout.contains("GLP-0002 :: active human (active)"));
    assert!(stdout.contains("GLP-0003 :: done human linked (done)"));
}

#[test]
fn e2e_list_status_filter_applies_before_cap_and_hidden_count() {
    let (_dir, cfg) = status_fixture_stage();
    let output = status_command_output(&cfg, &["list", "--status", "done", "-n", "1"]);
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(stdout.contains("CFG-0001 :: cfg done unlinked"), "{stdout}");
    assert!(!stdout.contains("GLP-0004"), "{stdout}");
    assert!(stdout.contains("1 more"), "{stdout}");
    assert!(!stdout.contains("active orphan"), "{stdout}");
}

#[test]
fn e2e_list_status_rejects_repeated_and_unknown_values() {
    let (_dir, cfg) = status_fixture_stage();
    pwf()
        .args(["list", "--status", "done", "--status", "active"])
        .arg("--config-path")
        .arg(&cfg)
        .assert()
        .failure()
        .stderr(contains("cannot be used multiple times"));
    pwf()
        .args(["list", "--status", "paused"])
        .arg("--config-path")
        .arg(&cfg)
        .assert()
        .failure()
        .stderr(contains("invalid value 'paused'"));
}

#[test]
fn e2e_list_status_rejects_duplicate_project_index_task_ids() {
    let (dir, cfg) = staged();
    let index_path = dir.path().join("notes/glep-shimeji/glep-shimeji.md");
    let mut index = fs::read_to_string(&index_path).unwrap();
    index.push_str("- [ ] [[GLP-0001|duplicate]]\n");
    fs::write(&index_path, index).unwrap();

    pwf()
        .args(["list", "--status", "all"])
        .arg("--config-path")
        .arg(&cfg)
        .assert()
        .failure()
        .stderr(contains("Project index task id GLP-0001 is duplicated"))
        .stderr(contains(index_path.to_string_lossy().as_ref()))
        .stderr(contains("lines 6, 7"));
}

#[test]
fn e2e_list_status_annotations_follow_color_environment_precedence() {
    let (_dir, cfg) = status_fixture_stage();
    let colored = pwf()
        .args(["list", "--status", "all", "-n", "0"])
        .arg("--config-path")
        .arg(&cfg)
        .env("CLICOLOR_FORCE", "1")
        .env_remove("NO_COLOR")
        .output()
        .unwrap();
    assert!(colored.status.success());
    let colored_stdout = String::from_utf8(colored.stdout).unwrap();
    assert!(
        colored_stdout.contains("\u{1b}[38;5;208mactive"),
        "{colored_stdout}"
    );
    assert!(
        colored_stdout.contains("\u{1b}[32mdone"),
        "{colored_stdout}"
    );
    assert!(
        colored_stdout.contains("\u{1b}[31mcancelled"),
        "{colored_stdout}"
    );

    let plain = pwf()
        .args(["list", "--status", "all", "-n", "0"])
        .arg("--config-path")
        .arg(&cfg)
        .env("CLICOLOR_FORCE", "1")
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(plain.status.success());
    let plain_stdout = String::from_utf8(plain.stdout).unwrap();
    assert!(!plain_stdout.contains('\u{1b}'), "{plain_stdout}");
    assert!(plain_stdout.contains("(active)"));
    assert!(plain_stdout.contains("(done)"));
    assert!(plain_stdout.contains("(cancelled)"));
}

#[test]
fn e2e_list_status_long_separates_lifecycle_and_launch_metadata() {
    let (_dir, cfg) = status_fixture_stage();
    let active = status_command_output(&cfg, &["list", "--status", "active", "--long"]);
    let active_stdout = String::from_utf8(active.stdout).unwrap();
    assert!(
        active_stdout.contains("  status: active\n"),
        "{active_stdout}"
    );
    assert!(
        active_stdout.contains("  launch: READY\n"),
        "{active_stdout}"
    );

    let done = status_command_output(&cfg, &["list", "--status", "done", "--long"]);
    let done_stdout = String::from_utf8(done.stdout).unwrap();
    assert!(done_stdout.contains("  status: done\n"), "{done_stdout}");
    assert!(!done_stdout.contains("launch:"), "{done_stdout}");
    assert!(!done_stdout.contains("issue:"), "{done_stdout}");
    assert!(!done_stdout.contains("fix:"), "{done_stdout}");
}

#[test]
fn list_default_caps_and_shows_more() {
    let (_d, cfg) = staged_many(12);
    pwf()
        .args(["list", "--config-path"])
        .arg(&cfg)
        .assert()
        .success()
        .stdout(contains("GLP-0012"))
        .stdout(contains("2 more"))
        .stdout(contains("-n 0"))
        .stdout(contains("GLP-0001").not());
}

#[test]
fn list_n_zero_shows_all() {
    let (_d, cfg) = staged_many(12);
    pwf()
        .args(["list", "-n", "0", "--config-path"])
        .arg(&cfg)
        .assert()
        .success()
        .stdout(contains("GLP-0001"))
        .stdout(contains("GLP-0012"))
        .stdout(contains("more").not());
}

#[test]
fn shorthand_project_forwards_number() {
    let (_d, cfg) = staged_many(12);
    pwf()
        .args(["glep-shimeji", "-n", "2", "--config-path"])
        .arg(&cfg)
        .assert()
        .success()
        .stdout(contains("GLP-0012"))
        .stdout(contains("GLP-0011"))
        .stdout(contains("GLP-0010").not());
}

#[test]
fn e2e_list_default_orders_by_created_desc_not_id() {
    let (_d, cfg) = staged_two_diverging_created();
    let out = pwf()
        .args(["list", "--config-path"])
        .arg(&cfg)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(out).unwrap();
    assert!(
        stdout.find("GLP-0001").unwrap() < stdout.find("GLP-0002").unwrap(),
        "expected newest-created (GLP-0001) first: {stdout}"
    );
}

#[test]
fn e2e_list_order_id_desc_reproduces_legacy_ordering() {
    let (_d, cfg) = staged_two_diverging_created();
    let out = pwf()
        .args(["list", "--order", "id", "desc", "--config-path"])
        .arg(&cfg)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(out).unwrap();
    assert!(
        stdout.find("GLP-0002").unwrap() < stdout.find("GLP-0001").unwrap(),
        "expected highest id (GLP-0002) first: {stdout}"
    );
}

#[test]
fn e2e_list_order_created_asc_orders_oldest_first() {
    let (_d, cfg) = staged_two_diverging_created();
    let out = pwf()
        .args(["list", "--order", "created", "asc", "--config-path"])
        .arg(&cfg)
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.find("GLP-0002").unwrap() < stdout.find("GLP-0001").unwrap(),
        "expected oldest-created (GLP-0002) first: {stdout}"
    );
}

#[test]
fn e2e_list_order_tokens_work_in_either_order() {
    let (_d, cfg) = staged_two_diverging_created();
    let a = pwf()
        .args(["list", "--order", "id", "asc", "--config-path"])
        .arg(&cfg)
        .output()
        .unwrap()
        .stdout;
    let b = pwf()
        .args(["list", "--order", "asc", "id", "--config-path"])
        .arg(&cfg)
        .output()
        .unwrap()
        .stdout;
    assert_eq!(a, b);
}

#[test]
fn e2e_list_order_rejects_conflicting_field_tokens() {
    let (_d, cfg) = staged_two();
    pwf()
        .args(["list", "--order", "created", "id", "--config-path"])
        .arg(&cfg)
        .assert()
        .failure()
        .stderr(contains("conflict"));
}

#[test]
fn e2e_list_order_rejects_unknown_token() {
    let (_d, cfg) = staged_two();
    pwf()
        .args(["list", "--order", "bogus", "--config-path"])
        .arg(&cfg)
        .assert()
        .failure()
        .stderr(contains("invalid value"));
}

#[test]
fn e2e_list_default_across_all_projects_is_flat_by_created_not_grouped_by_project() {
    let (_d, cfg) = staged_two_projects_diverging_created();
    let out = pwf()
        .args(["list", "--config-path"])
        .arg(&cfg)
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.find("GLP-0099").unwrap() < stdout.find("CFG-0001").unwrap(),
        "expected the newer item (GLP-0099) first, ignoring project grouping: {stdout}"
    );
}

#[test]
fn e2e_list_order_project_id_reproduces_legacy_grouped_default() {
    let (_d, cfg) = staged_two_projects_diverging_created();
    let out = pwf()
        .args(["list", "--order", "project-id", "--config-path"])
        .arg(&cfg)
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.find("CFG-0001").unwrap() < stdout.find("GLP-0099").unwrap(),
        "--order project-id must group by project ascending, regardless of created date: {stdout}"
    );
}

#[test]
fn e2e_route_project_shorthand_ignores_created_stays_id_desc() {
    let (_d, cfg) = staged_two_diverging_created();
    let out = pwf()
        .args(["glep-shimeji", "--config-path"])
        .arg(&cfg)
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.find("GLP-0002").unwrap() < stdout.find("GLP-0001").unwrap(),
        "route shorthand must stay id-descending: {stdout}"
    );
}

#[test]
fn retired_legacy_flag_surface_errors() {
    // Legacy `-Action` tokens route as words and must never trigger a list.
    let (_d, cfg) = staged();
    pwf()
        .args(["-Action", "list", "-ConfigPath"])
        .arg(&cfg)
        .assert()
        .failure();
}

#[test]
fn canonical_only_prereq_flag_works() {
    let (_d, cfg) = staged();
    pwf()
        .args([
            "add",
            "glep-shimeji",
            "do the thing",
            "--prereq",
            "GLP-0001",
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .success();
}

fn read_item(dir: &TempDir, id: &str) -> String {
    fs::read_to_string(dir.path().join(format!("notes/glep-shimeji/{id}.md"))).unwrap()
}

#[test]
fn e2e_add_tags_write_canonical_frontmatter() {
    let (d, cfg) = staged();
    pwf()
        .args([
            "add",
            "glep-shimeji",
            "tagged task",
            "--tag",
            "SQLite,csharp-export",
            "--tag",
            "godot",
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .success();
    let item = read_item(&d, "GLP-0002");
    assert!(
        item.contains("tags: [sqlite, csharp_export, godot]\n"),
        "{item}"
    );
}

#[test]
fn e2e_list_tag_filter_requires_all_requested_tags() {
    let (_d, cfg) = staged_tagged_items();
    pwf()
        .args(["list", "--tag", "SQLite,godot", "--config-path"])
        .arg(&cfg)
        .assert()
        .success()
        .stdout(contains("both tags"))
        .stdout(contains("sqlite only").not())
        .stdout(contains("untagged").not());
}

#[test]
fn e2e_list_long_displays_raw_tags_without_parsing() {
    let (d, cfg) = staged_tagged_items();
    let item_path = d.path().join("notes/glep-shimeji/GLP-0001.md");
    fs::write(
        item_path,
        "---\nid: GLP-0001\nstatus: active\ntitle: both tags\nproject: glep-shimeji\ncreated: 2026-01-01\ntags: SQLite,godot\n---\n\nbody\n",
    )
    .unwrap();
    pwf()
        .args(["list", "--long", "--config-path"])
        .arg(&cfg)
        .assert()
        .success()
        .stdout(contains("tags: SQLite,godot"));
}

#[test]
fn e2e_list_tag_filter_rejects_corrupt_frontmatter() {
    let (d, cfg) = staged_tagged_items();
    let item_path = d.path().join("notes/glep-shimeji/GLP-0001.md");
    fs::write(
        item_path,
        "---\nid: GLP-0001\nstatus: active\ntitle: both tags\nproject: glep-shimeji\ncreated: 2026-01-01\ntags: sqlite,godot\n---\n\nbody\n",
    )
    .unwrap();
    pwf()
        .args(["list", "--tag", "sqlite", "--config-path"])
        .arg(&cfg)
        .assert()
        .failure()
        .stderr(contains("invalid tags frontmatter").and(contains("GLP-0001")));
}

#[test]
fn e2e_list_tag_filter_rejects_empty_tags_frontmatter_with_item_context() {
    let (d, cfg) = staged_tagged_items();
    let item_path = d.path().join("notes/glep-shimeji/GLP-0001.md");
    fs::write(
        item_path,
        "---\nid: GLP-0001\nstatus: active\ntitle: both tags\nproject: glep-shimeji\ncreated: 2026-01-01\ntags:   \n---\n\nbody\n",
    )
    .unwrap();
    pwf()
        .args(["list", "--tag", "sqlite", "--config-path"])
        .arg(&cfg)
        .assert()
        .failure()
        .stderr(contains("item GLP-0001 has invalid tags frontmatter"));
}

#[test]
fn e2e_invalid_list_leading_hyphen_tag_names_raw_value() {
    let (_d, cfg) = staged_tagged_items();
    pwf()
        .args([
            "list",
            "--tag",
            LEADING_HYPHEN_TAG,
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .failure()
        .stderr(contains(
            r#"Error: Invalid --tag value "-sqlite"; use lowercase/uppercase ASCII letters, digits, '_' or '-', without leading, trailing, or repeated separators."#,
        ));
}

#[test]
fn e2e_update_tags_append_deduplicate_clear_and_replace() {
    let (d, cfg) = staged();
    let item_path = d.path().join("notes/glep-shimeji/GLP-0001.md");
    fs::write(
        item_path,
        "---\nid: GLP-0001\nstatus: active\ntitle: tray gui\nproject: glep-shimeji\ncreated: 2026-01-01\ntags: [sqlite, godot]\n---\n\nadd toggle\n",
    )
    .unwrap();
    pwf()
        .args([
            "update",
            "GLP-0001",
            "--tag",
            "godot,csharp-export",
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .success();
    let item = read_item(&d, "GLP-0001");
    assert!(
        item.contains("tags: [sqlite, godot, csharp_export]\n"),
        "{item}"
    );

    pwf()
        .args([
            "update",
            "GLP-0001",
            "--tags-clear",
            "--tag",
            "setup",
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .success();
    let item = read_item(&d, "GLP-0001");
    assert!(item.contains("tags: [setup]\n"), "{item}");
    assert!(!item.contains("sqlite"), "{item}");
}

#[test]
fn e2e_invalid_add_tag_names_raw_value_and_writes_nothing() {
    let (d, cfg) = staged();
    pwf()
        .args([
            "add",
            "glep-shimeji",
            "x",
            "--tag",
            "sqlite__export",
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .failure()
        .stderr(contains("--tag").and(contains("sqlite__export")));
    assert!(!d.path().join("notes/glep-shimeji/GLP-0002.md").exists());
}

#[test]
fn e2e_invalid_add_leading_hyphen_tag_names_raw_value_and_writes_nothing() {
    let (d, cfg) = staged();
    pwf()
        .args([
            "add",
            "glep-shimeji",
            "x",
            "--tag",
            LEADING_HYPHEN_TAG,
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .failure()
        .stderr(contains("--tag").and(contains(LEADING_HYPHEN_TAG)));
    assert!(!d.path().join("notes/glep-shimeji/GLP-0002.md").exists());
}

#[test]
fn e2e_invalid_update_leading_hyphen_tag_names_raw_value_and_writes_nothing() {
    let (d, cfg) = staged();
    let item_path = d.path().join("notes/glep-shimeji/GLP-0001.md");
    let before = fs::read_to_string(&item_path).unwrap();
    pwf()
        .args([
            "update",
            "GLP-0001",
            "--tag",
            LEADING_HYPHEN_TAG,
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .failure()
        .stderr(contains("--tag").and(contains(LEADING_HYPHEN_TAG)));
    assert_eq!(fs::read_to_string(item_path).unwrap(), before);
}

#[test]
fn e2e_update_tags_clear_is_idempotent_on_untagged_item() {
    let (d, cfg) = staged();
    pwf()
        .args(["update", "GLP-0001", "--tags-clear", "--config-path"])
        .arg(&cfg)
        .assert()
        .success();
    assert!(!read_item(&d, "GLP-0001").contains("tags:"));
}

#[test]
fn e2e_update_nothing_to_update_mentions_tag_flags() {
    let (_d, cfg) = staged();
    pwf()
        .args(["update", "GLP-0001", "--config-path"])
        .arg(&cfg)
        .assert()
        .failure()
        .stderr(contains("--tag").and(contains("--tags-clear")));
}

#[test]
fn e2e_update_closed_item_rejects_tag_edits_without_writing() {
    let (d, cfg) = staged();
    let project = d.path().join("notes/glep-shimeji");
    fs::write(
        project.join("glep-shimeji.md"),
        "---\nid: glp\ntitle: glep-shimeji\n---\n\n- [x] [[GLP-0001|tray gui]] ✅ 2026-01-02\n",
    )
    .unwrap();
    let item_path = project.join("GLP-0001.md");
    fs::write(
        &item_path,
        "---\nid: GLP-0001\nstatus: done\ntitle: tray gui\nproject: glep-shimeji\ncreated: 2026-01-01\ncompleted: 2026-01-02\n---\n\nadd toggle\n",
    )
    .unwrap();
    let before = fs::read_to_string(&item_path).unwrap();
    pwf()
        .args(["update", "GLP-0001", "--tag", "sqlite", "--config-path"])
        .arg(&cfg)
        .assert()
        .failure()
        .stderr(contains("tags").and(contains("open item")));
    assert_eq!(fs::read_to_string(item_path).unwrap(), before);
}

fn title_of(item: &str) -> &str {
    item.lines()
        .find_map(|l| l.strip_prefix("title: "))
        .expect("title frontmatter present")
}

#[test]
fn e2e_update_prompt_rewrites_body_and_preserves_frontmatter() {
    let (d, cfg) = staged();
    pwf()
        .args([
            "update",
            "--id",
            "GLP-0001",
            "--prompt",
            "a / b /c context /n no manual edit /d tests pass",
            "--date",
            "2026-01-01",
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .success();
    let item = read_item(&d, "GLP-0001");
    assert!(
        item.contains("## Goals\n- a\n- b"),
        "body not Goals-wrapped: {item}"
    );
    assert!(item.contains("## Context\n- context"), "context: {item}");
    assert!(
        item.contains("## Constraints\n- no manual edit"),
        "constraints: {item}"
    );
    assert!(
        item.contains("## Done When\n- tests pass"),
        "done when: {item}"
    );
    assert!(!item.contains("add toggle"), "old body replaced: {item}");
    assert!(item.contains("status: active"));
    assert!(item.contains("title: tray gui"));
    assert!(item.contains("project: glep-shimeji"));
    assert!(item.contains("created: 2026-01-01"));
    assert!(!d.path().join("notes/glep-shimeji/GLP-0001.md.bak").exists());
}

#[test]
fn e2e_update_title_only_leaves_body_untouched() {
    let (d, cfg) = staged();
    pwf()
        .args([
            "update",
            "--id",
            "GLP-0001",
            "--title",
            "X",
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .success();
    let item = read_item(&d, "GLP-0001");
    assert_eq!(title_of(&item), "x", "title replaced+normalized: {item}");
    assert!(item.contains("add toggle"), "body untouched: {item}");
}

#[test]
fn e2e_add_normalizes_colon_title_and_notes_it_on_stderr() {
    let (d, cfg) = staged();
    pwf()
        .args([
            "add",
            "glep-shimeji",
            "prompt body",
            "--title",
            "finish refactor: promote sync-git seam",
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .success()
        .stderr(contains("info: title normalized to keep metadata valid"));
    let item = read_item(&d, "GLP-0002");
    assert_eq!(
        title_of(&item),
        "finish refactor; promote sync-git seam",
        "title not yaml-safe: {item}"
    );
    pwf()
        .args(["show", "GLP-0002", "--config-path"])
        .arg(&cfg)
        .assert()
        .success()
        .stdout(contains("finish refactor; promote sync-git seam"));
}

#[test]
fn e2e_add_safe_title_emits_no_normalization_notice() {
    let (d, cfg) = staged();
    pwf()
        .args([
            "add",
            "glep-shimeji",
            "prompt body",
            "--title",
            "Plain Safe Title",
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .success()
        .stderr(contains("title normalized").not());
    assert_eq!(title_of(&read_item(&d, "GLP-0002")), "plain safe title");
}

#[test]
fn e2e_add_inferred_colon_title_normalizes_silently() {
    let (d, cfg) = staged();
    pwf()
        .args([
            "add",
            "glep-shimeji",
            "fix bug: empty prompt",
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .success()
        .stderr(contains("title normalized").not());
    assert_eq!(
        title_of(&read_item(&d, "GLP-0002")),
        "fix bug; empty prompt"
    );
}

#[test]
fn e2e_update_normalizes_colon_title_and_notes_it_on_stderr() {
    let (d, cfg) = staged();
    pwf()
        .args([
            "update",
            "--id",
            "GLP-0001",
            "--title",
            "fix bug: handle colons",
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .success()
        .stderr(contains("info: title normalized to keep metadata valid"));
    let item = read_item(&d, "GLP-0001");
    assert_eq!(title_of(&item), "fix bug; handle colons");
    pwf()
        .args(["show", "GLP-0001", "--config-path"])
        .arg(&cfg)
        .assert()
        .success()
        .stdout(contains("fix bug; handle colons"));
}

#[test]
fn e2e_update_prompt_only_leaves_title_untouched() {
    let (d, cfg) = staged();
    pwf()
        .args([
            "update",
            "--id",
            "GLP-0001",
            "--prompt",
            "fresh prompt",
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .success();
    let item = read_item(&d, "GLP-0001");
    assert_eq!(title_of(&item), "tray gui", "title untouched: {item}");
    assert!(item.contains("## Goals\n- fresh prompt"), "body: {item}");
}

#[test]
fn e2e_update_requires_a_field() {
    let (_d, cfg) = staged();
    pwf()
        .args(["update", "--id", "GLP-0001", "--config-path"])
        .arg(&cfg)
        .assert()
        .failure();
}

#[test]
fn e2e_update_unknown_id_fails() {
    let (_d, cfg) = staged();
    pwf()
        .args([
            "update",
            "--id",
            "GLP-9999",
            "--prompt",
            "x",
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .failure();
}

#[test]
fn e2e_add_rich_prompt_lanes_render_sections() {
    let (d, cfg) = staged();
    pwf()
        .args([
            "add",
            "glep-shimeji",
            "lead clause / goal two / goal three /c context one /n no parser crate /d tests pass",
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .success();
    let item = read_item(&d, "GLP-0002");
    assert_eq!(title_of(&item), "lead clause", "title not cut: {item}");
    assert!(
        item.contains("## Goals\n- lead clause\n- goal two\n- goal three"),
        "goals not rendered: {item}"
    );
    assert!(
        item.contains("## Context\n- context one"),
        "context not rendered: {item}"
    );
    assert!(
        item.contains("## Constraints\n- no parser crate"),
        "constraints not rendered: {item}"
    );
    assert!(
        item.contains("## Done When\n- tests pass"),
        "done-when not rendered: {item}"
    );
}

#[test]
fn e2e_add_marker_first_prompt_defaults_title_without_body_sentinel() {
    let (d, cfg) = staged();
    pwf()
        .args(["add", "glep-shimeji", "/c context", "--config-path"])
        .arg(&cfg)
        .assert()
        .success();

    let item = read_item(&d, "GLP-0002");
    assert_eq!(title_of(&item), "n/a", "missing title fallback: {item}");
    assert!(
        item.contains("## Context\n- context"),
        "authored context missing: {item}"
    );
    let body = item.split_once("---\n\n").unwrap().1;
    assert!(!body.contains("n/a"), "fallback leaked into body: {item}");
    assert!(
        !body.contains("pending work"),
        "legacy fallback leaked into body: {item}"
    );
    assert!(
        !body.contains("\n- \n"),
        "empty goal leaked into body: {item}"
    );
}

#[test]
fn e2e_add_caps_long_title_without_ampersand() {
    let (d, cfg) = staged();
    let long = "Continue the PowerShell to Rust port into the cfgtool CLI using the shipped gaming domain as the template porting smallest first";
    pwf()
        .args(["add", "glep-shimeji", long, "--config-path"])
        .arg(&cfg)
        .assert()
        .success();
    let item = read_item(&d, "GLP-0002");
    let title = title_of(&item);
    // The ellipsis adds one character to the 80-character title cap.
    assert!(
        title.chars().count() <= 81,
        "title must stay bounded, got {}: {title}",
        title.chars().count()
    );
    assert!(
        title.ends_with('…'),
        "truncated title carries ellipsis: {title}"
    );
    assert!(
        !title.contains("smallest first"),
        "tail dropped from title: {title}"
    );
    assert!(
        item.contains("smallest first"),
        "body keeps full prompt: {item}"
    );
}

#[test]
fn e2e_add_lowercases_inferred_title() {
    let (d, cfg) = staged();
    pwf()
        .args([
            "add",
            "glep-shimeji",
            "Refactor Help Command",
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .success();
    let item = read_item(&d, "GLP-0002");
    assert_eq!(title_of(&item), "refactor help command", "inferred: {item}");
}

#[test]
fn e2e_add_lowercases_explicit_title() {
    let (d, cfg) = staged();
    pwf()
        .args([
            "add",
            "glep-shimeji",
            "x",
            "--title",
            "UPPER THING",
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .success();
    let item = read_item(&d, "GLP-0002");
    assert_eq!(title_of(&item), "upper thing", "explicit add: {item}");
}

#[test]
fn e2e_update_lowercases_explicit_title() {
    let (d, cfg) = staged();
    pwf()
        .args([
            "update",
            "--id",
            "GLP-0001",
            "--title",
            "UPPER THING",
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .success();
    let item = read_item(&d, "GLP-0001");
    assert_eq!(title_of(&item), "upper thing", "explicit update: {item}");
}

#[test]
fn e2e_done_rotates_done_queue_past_general_cap() {
    let (d, cfg) = staged_many(7);
    for n in 1..=7 {
        let id = format!("GLP-{n:04}");
        pwf()
            .args(["done", "--id", &id, "--date", "2026-01-01", "--config-path"])
            .arg(&cfg)
            .assert()
            .success();
    }
    let index = read_index(&d);
    assert!(
        index.contains("- [x] [[GLP-0007]] ✅ 2026-01-01"),
        "checked item not marked in place: {index}"
    );
    assert_eq!(
        index.matches("- [x]").count(),
        6,
        "cap not enforced: {index}"
    );
    assert!(!index.contains("GLP-0001"), "oldest not evicted: {index}");
    assert!(d.path().join("notes/glep-shimeji/GLP-0001.md").exists());
    assert!(!d.path().join("notes/glep-shimeji/_archive").exists());
    assert!(
        !d.path()
            .join("notes/glep-shimeji/glep-shimeji.md.bak")
            .exists()
    );
}

#[test]
fn e2e_add_with_prereq_writes_validated_frontmatter() {
    let (d, cfg) = staged();
    pwf()
        .args([
            "add",
            "glep-shimeji",
            "x",
            "--prereq",
            "GLP-0001",
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .success();
    let item = read_item(&d, "GLP-0002");
    assert!(
        item.contains("prereq: \"[[GLP-0001]]\""),
        "prereq frontmatter missing: {item}"
    );
}

#[test]
fn e2e_add_with_prereq_shorthand_normalizes_to_canonical() {
    let (d, cfg) = staged();
    pwf()
        .args([
            "add",
            "glep-shimeji",
            "x",
            "--prereq",
            "glp1",
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .success();
    let item = read_item(&d, "GLP-0002");
    assert!(
        item.contains("prereq: \"[[GLP-0001]]\""),
        "shorthand prereq not canonicalized: {item}"
    );
}

#[test]
fn e2e_add_with_effort_writes_frontmatter() {
    let (d, cfg) = staged();
    pwf()
        .args(["add", "glep-shimeji", "x", "--effort", "3", "--config-path"])
        .arg(&cfg)
        .assert()
        .success();
    let item = read_item(&d, "GLP-0002");
    assert!(
        item.contains("effort: 3\n"),
        "effort frontmatter missing: {item}"
    );
}

#[test]
fn e2e_add_effort_out_of_range_is_rejected() {
    let (d, cfg) = staged();
    pwf()
        .args(["add", "glep-shimeji", "x", "--effort", "5", "--config-path"])
        .arg(&cfg)
        .assert()
        .failure();
    assert!(
        !d.path().join("notes/glep-shimeji/GLP-0002.md").exists(),
        "failed add wrote a new item"
    );
}

#[test]
fn e2e_add_rejects_unknown_prereq_without_writing_item() {
    let (d, cfg) = staged();
    pwf()
        .args([
            "add",
            "glep-shimeji",
            "x",
            "--prereq",
            "GLP-9999",
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .failure();
    assert!(
        !d.path().join("notes/glep-shimeji/GLP-0002.md").exists(),
        "failed add wrote a new item"
    );
}

#[test]
fn e2e_update_prereq_writes_validated_frontmatter() {
    let (d, cfg) = staged_two();
    pwf()
        .args([
            "update",
            "--id",
            "GLP-0002",
            "--prereq",
            "GLP-0001",
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .success();
    let item = read_item(&d, "GLP-0002");
    assert!(
        item.contains("prereq: \"[[GLP-0001]]\""),
        "prereq frontmatter missing: {item}"
    );
}

#[test]
fn e2e_update_effort_writes_frontmatter() {
    let (d, cfg) = staged();
    pwf()
        .args([
            "update",
            "--id",
            "GLP-0001",
            "--effort",
            "4",
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .success();
    let item = read_item(&d, "GLP-0001");
    assert!(
        item.contains("effort: 4\n"),
        "effort frontmatter missing: {item}"
    );
}

#[test]
fn e2e_list_effort_filter_shows_only_matching_tier() {
    let (_d, cfg) = staged_two();
    pwf()
        .args([
            "update",
            "--id",
            "GLP-0001",
            "--effort",
            "1",
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .success();
    pwf()
        .args([
            "update",
            "--id",
            "GLP-0002",
            "--effort",
            "4",
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .success();

    pwf()
        .args(["list", "--effort", "4", "--config-path"])
        .arg(&cfg)
        .assert()
        .success()
        .stdout(contains("second"))
        .stdout(contains("tray gui").not());
}

#[test]
fn e2e_list_long_shows_effort_line() {
    let (_d, cfg) = staged();
    pwf()
        .args([
            "update",
            "--id",
            "GLP-0001",
            "--effort",
            "2",
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .success();

    pwf()
        .args(["list", "--long", "--config-path"])
        .arg(&cfg)
        .assert()
        .success()
        .stdout(contains("effort: 2"));
}

#[test]
fn e2e_update_prereq_appends_and_dedups() {
    let (d, cfg) = staged_two();
    // Seed an existing prerequisite to exercise append deduplication.
    let proj = d.path().join("notes/glep-shimeji");
    fs::write(
        proj.join("GLP-0002.md"),
        "---\nid: GLP-0002\nstatus: active\ntitle: second\nproject: glep-shimeji\ncreated: 2026-01-02\nprereq: \"[[GLP-0001]]\"\n---\n\ndo more\n",
    )
    .unwrap();
    pwf()
        .args([
            "update",
            "--id",
            "GLP-0002",
            "--prereq",
            "GLP-0001,GLP-0001",
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .success();
    let item = read_item(&d, "GLP-0002");
    assert_eq!(
        item.matches("[[GLP-0001]]").count(),
        1,
        "prereq duplicated: {item}"
    );
    assert_eq!(item.matches("prereq:").count(), 1, "duplicate line: {item}");
}

#[test]
fn e2e_update_clear_prereq_empties_it() {
    let (d, cfg) = staged_two();
    let proj = d.path().join("notes/glep-shimeji");
    fs::write(
        proj.join("GLP-0002.md"),
        "---\nid: GLP-0002\nstatus: active\ntitle: second\nproject: glep-shimeji\ncreated: 2026-01-02\nprereq: \"[[GLP-0001]]\"\n---\n\ndo more\n",
    )
    .unwrap();
    pwf()
        .args([
            "update",
            "--id",
            "GLP-0002",
            "--clear-prereq",
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .success();
    let item = read_item(&d, "GLP-0002");
    assert!(!item.contains("prereq:"), "prereq line lingered: {item}");
    assert!(item.contains("status: active"));
    assert!(item.contains("title: second"));
    assert!(item.contains("created: 2026-01-02"));
}

#[test]
fn e2e_update_prereq_rejects_unknown() {
    let (d, cfg) = staged_two();
    let before = read_item(&d, "GLP-0002");
    pwf()
        .args([
            "update",
            "--id",
            "GLP-0002",
            "--prereq",
            "GLP-9999",
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .failure()
        .stderr(contains("GLP-9999"));
    assert_eq!(read_item(&d, "GLP-0002"), before, "item changed on failure");
}

#[test]
fn e2e_update_prereq_and_clear_conflict() {
    let (_d, cfg) = staged_two();
    pwf()
        .args([
            "update",
            "--id",
            "GLP-0002",
            "--prereq",
            "GLP-0001",
            "--clear-prereq",
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .failure();
}

#[test]
fn e2e_add_default_section_lands_before_any_header() {
    let (d, cfg) = staged();
    // Seed a section header so the test can distinguish top-level placement.
    let index_path = d.path().join("notes/glep-shimeji/glep-shimeji.md");
    let seeded = format!(
        "{}\n## Future\n- [ ] [[GLP-0099|future thing]]\n",
        read_index(&d)
    );
    fs::write(&index_path, seeded).unwrap();
    pwf()
        .args([
            "add",
            "glep-shimeji",
            "default placed item",
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .success();
    let index = read_index(&d);
    let item = index.find("[[GLP-0002]]").expect("no new item link");
    let header = index.find("## ").expect("no `## ` header in index");
    assert!(item < header, "item not before first header: {index}");
}

#[test]
fn e2e_done_normalizes_mixed_case_id() {
    let (d, cfg) = staged();
    pwf()
        .args([
            "done",
            "--id",
            "glp-0001",
            "--date",
            "2026-01-01",
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .success();
    let item = read_item(&d, "GLP-0001");
    assert!(item.contains("status: done"), "item not checked: {item}");
    let index = read_index(&d);
    assert!(
        index.contains("[x] [[GLP-0001]]"),
        "index did not use canonical id: {index}"
    );
}

#[test]
fn e2e_done_commits_writes_provenance_frontmatter() {
    let (d, cfg) = staged();
    pwf()
        .args([
            "done",
            "--id",
            "GLP-0001",
            "--commits",
            "a1b2c3d..f4e5d6c",
            "--date",
            "2026-01-01",
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .success();
    let item = read_item(&d, "GLP-0001");
    assert!(
        item.contains("commits: \"a1b2c3d..f4e5d6c\""),
        "commits frontmatter missing: {item}"
    );
}

#[test]
fn e2e_done_commits_repeated_and_comma_join_and_dedup() {
    let (d, cfg) = staged();
    pwf()
        .args([
            "done",
            "--id",
            "GLP-0001",
            "--commits",
            "a..b",
            "--commits",
            "c..d",
            "--date",
            "2026-01-01",
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .success();
    assert!(
        read_item(&d, "GLP-0001").contains("commits: \"a..b, c..d\""),
        "repeated commits not joined"
    );

    let (d2, cfg2) = staged();
    pwf()
        .args([
            "done",
            "--id",
            "GLP-0001",
            "--commits",
            "a..b,c..d",
            "--date",
            "2026-01-01",
            "--config-path",
        ])
        .arg(&cfg2)
        .assert()
        .success();
    assert!(
        read_item(&d2, "GLP-0001").contains("commits: \"a..b, c..d\""),
        "comma commits not joined"
    );
}

#[test]
fn e2e_done_without_commits_writes_no_commits_line() {
    let (d, cfg) = staged();
    pwf()
        .args([
            "done",
            "--id",
            "GLP-0001",
            "--date",
            "2026-01-01",
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .success();
    assert!(
        !read_item(&d, "GLP-0001").contains("commits:"),
        "default done path leaked a commits line"
    );
}

#[test]
fn e2e_done_review_spawns_human_task_scoped_to_range() {
    let (d, cfg) = staged();
    let out = pwf()
        .args([
            "done",
            "--id",
            "GLP-0001",
            "--commits",
            "a..b",
            "--review",
            "--date",
            "2026-01-01",
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .success()
        .stderr(contains(
            "info: created `## Human` section in glep-shimeji\n",
        ));
    assert!(
        read_item(&d, "GLP-0001").contains("commits: \"a..b\""),
        "checked item missing commits"
    );
    let index = read_index(&d);
    assert!(index.contains("## Human"), "no Human section: {index}");
    let spawned = read_item(&d, "GLP-0002");
    assert_eq!(
        spawned,
        "---\nid: GLP-0002\nstatus: active\ntitle: review glp-0001, commits; a..b\nproject: glep-shimeji\ncreated: 2026-01-01\n---\n\n## Goals\n- review GLP-0001, commits: a..b\n- git-tools diff a..b\n- git-tools diff-subrepos\n"
    );
    drop(out);
}

#[test]
fn e2e_done_review_appends_review_task_as_text() {
    let (_d, cfg) = staged();
    let out = pwf()
        .args([
            "done",
            "--id",
            "GLP-0001",
            "--commits",
            "a..b",
            "--review",
            "--date",
            "2026-01-01",
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.starts_with("Done GLP-0001"),
        "expected text output: {stdout}"
    );
    assert!(
        stdout.contains("ADDED PWF TASK [GLP-0002]"),
        "review task appended as text: {stdout}"
    );
}

#[test]
fn e2e_done_review_without_commits_uses_bare_diff_fallback() {
    let (d, cfg) = staged();
    pwf()
        .args([
            "done",
            "--id",
            "GLP-0001",
            "--review",
            "--date",
            "2026-01-01",
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .success();
    let spawned = read_item(&d, "GLP-0002");
    assert_eq!(
        spawned,
        "---\nid: GLP-0002\nstatus: active\ntitle: review glp-0001\nproject: glep-shimeji\ncreated: 2026-01-01\n---\n\n## Goals\n- review GLP-0001\n- git-tools diff\n- git-tools diff-subrepos\n"
    );
}

/// Stages one custom item with a canonical open index link.
fn staged_with_item(
    project: &str,
    prefix: &str,
    id: &str,
    title: &str,
    content: &str,
) -> (TempDir, std::path::PathBuf) {
    let dir = TempDir::new().unwrap();
    let notes = dir.path().join("notes");
    let proj = notes.join(project);
    fs::create_dir_all(&proj).unwrap();
    fs::write(proj.join(format!("{id}.md")), content).unwrap();
    fs::write(
        proj.join(format!("{project}.md")),
        format!("- [ ] [[{id}|{title}]]\n"),
    )
    .unwrap();
    let cfg = dir.path().join("cfg.json");
    fs::write(
        &cfg,
        format!(
            r#"{{ "notesDir": {:?}, "projects": {{ {:?}: "/repo" }}, "prefixes": {{ {:?}: {:?} }} }}"#,
            notes.to_string_lossy(),
            project,
            project,
            prefix
        ),
    )
    .unwrap();
    finish_fixture(dir, cfg)
}

#[test]
fn show_emits_markdown_with_created_key() {
    let (_d, cfg) = staged_with_item(
        "pwf",
        "PWF",
        "PWF-0001",
        "do the thing",
        "---\nid: PWF-0001\nstatus: active\ntitle: do the thing\nproject: pwf\ncreated: 2026-06-20\n---\n\n## Goals\n- do the thing\n",
    );
    let out = pwf()
        .args(["show", "--id", "PWF-0001", "--config-path"])
        .arg(&cfg)
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.contains("status: active"),
        "missing status: {stdout}"
    );
    assert!(
        stdout.contains("title: do the thing"),
        "missing title: {stdout}"
    );
    assert!(stdout.contains("## Goals"), "missing body: {stdout}");
    assert!(stdout.contains("- do the thing"), "missing goal: {stdout}");
    assert!(
        stdout.contains("created: 2026-06-20"),
        "created date must be shown: {stdout}"
    );
}

#[test]
fn show_legacy_item_emits_body_only() {
    // Legacy inline items have no note file, so show falls back to their parsed prompt.
    let dir = TempDir::new().unwrap();
    let notes = dir.path().join("notes");
    let proj = notes.join("glep-shimeji");
    fs::create_dir_all(&proj).unwrap();
    fs::write(
        proj.join("glep-shimeji.md"),
        "---\nid: glp\ntitle: glep-shimeji\n---\n\n- [ ] `legacy task` <- do the legacy thing\n",
    )
    .unwrap();
    let cfg = dir.path().join("cfg.json");
    fs::write(
        &cfg,
        format!(
            r#"{{ "notesDir": {:?}, "projects": {{ "glep-shimeji": "/repo" }}, "prefixes": {{ "glep-shimeji": "GLP" }} }}"#,
            notes.to_string_lossy()
        ),
    )
    .unwrap();
    let out = pwf()
        .args(["show", "--id", "glep-shimeji:1", "--config-path"])
        .arg(&cfg)
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.contains("do the legacy thing"),
        "prompt not in output: {stdout}"
    );
}

/// Stages one archived item without an index entry.
fn staged_with_archived_item(
    project: &str,
    prefix: &str,
    id: &str,
    content: &str,
) -> (TempDir, std::path::PathBuf) {
    let dir = TempDir::new().unwrap();
    let notes = dir.path().join("notes");
    let project_dir = notes.join(project);
    fs::create_dir_all(&project_dir).unwrap();
    fs::write(project_dir.join(format!("{id}.md")), content).unwrap();
    let cfg = dir.path().join("cfg.json");
    fs::write(
        &cfg,
        format!(
            r#"{{ "notesDir": {:?}, "projects": {{ {:?}: "/repo" }}, "prefixes": {{ {:?}: {:?} }} }}"#,
            notes.to_string_lossy(),
            project,
            project,
            prefix
        ),
    )
    .unwrap();
    finish_fixture(dir, cfg)
}

/// Stages a done item whose note remains in place behind a checked index link.
fn staged_with_done_item(
    project: &str,
    prefix: &str,
    id: &str,
    content: &str,
) -> (TempDir, std::path::PathBuf) {
    let dir = TempDir::new().unwrap();
    let notes = dir.path().join("notes");
    let proj = notes.join(project);
    fs::create_dir_all(&proj).unwrap();
    fs::write(proj.join(format!("{id}.md")), content).unwrap();
    fs::write(
        proj.join(format!("{project}.md")),
        format!("- [x] [[{id}]] ✅ 2026-06-20\n"),
    )
    .unwrap();
    let cfg = dir.path().join("cfg.json");
    fs::write(
        &cfg,
        format!(
            r#"{{ "notesDir": {:?}, "projects": {{ {:?}: "/repo" }}, "prefixes": {{ {:?}: {:?} }} }}"#,
            notes.to_string_lossy(),
            project,
            project,
            prefix
        ),
    )
    .unwrap();
    finish_fixture(dir, cfg)
}

#[test]
fn e2e_reopen_flips_done_item_back_to_active_and_restores_index() {
    let (dir, cfg) = staged_with_done_item(
        "pwf",
        "PWF",
        "PWF-0003",
        "---\nid: PWF-0003\nstatus: done\ntitle: just done\nproject: pwf\ncreated: 2026-06-20\ncompleted: 2026-06-20\ncommits: \"a..b\"\n---\n\n## Goals\n- finish it\n",
    );
    pwf()
        .args(["reopen", "--id", "pwf-0003", "--config-path"])
        .arg(&cfg)
        .assert()
        .success()
        .stdout(contains("Reopened PWF-0003"));

    let note = fs::read_to_string(dir.path().join("notes/pwf/PWF-0003.md")).unwrap();
    assert!(note.contains("status: active"), "status: {note}");
    assert!(!note.contains("completed:"), "completed lingered: {note}");
    assert!(!note.contains("commits:"), "commits lingered: {note}");
    let index = fs::read_to_string(dir.path().join("notes/pwf/pwf.md")).unwrap();
    assert_eq!(
        index, "---\nid: pwf\ntitle: pwf\n---\n\n- [ ] [[PWF-0003]]\n",
        "index not reopened: {index}"
    );
}

#[test]
fn e2e_reopen_already_active_item_skips() {
    let (_d, cfg) = staged();
    pwf()
        .args(["reopen", "--id", "GLP-0001", "--config-path"])
        .arg(&cfg)
        .assert()
        .success()
        .stdout(contains("already active"));
}

#[test]
fn e2e_reopen_unknown_id_errors() {
    let (_d, cfg) = staged();
    pwf()
        .args(["reopen", "--id", "GLP-9999", "--config-path"])
        .arg(&cfg)
        .assert()
        .failure()
        .stderr(contains("not found"));
}

#[test]
fn show_finds_done_item_still_in_project_dir() {
    let (_d, cfg) = staged_with_done_item(
        "pwf",
        "PWF",
        "PWF-0003",
        "---\nstatus: done\ntitle: just done\nproject: pwf\ncompleted: 2026-06-20\n---\n\n## Goals\n- just done\n",
    );
    let out = pwf()
        .args(["show", "--id", "PWF-0003", "--config-path"])
        .arg(&cfg)
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("status: done"), "missing status: {stdout}");
    assert!(stdout.contains("- just done"), "missing body: {stdout}");
}

#[test]
fn show_finds_done_item_still_in_project_dir_with_shorthand_id() {
    let (_d, cfg) = staged_with_done_item(
        "pwf",
        "PWF",
        "PWF-0003",
        "---\nstatus: done\ntitle: just done\nproject: pwf\ncompleted: 2026-06-20\n---\n\n## Goals\n- just done\n",
    );
    let out = pwf()
        .args(["show", "--id", "pwf3", "--config-path"])
        .arg(&cfg)
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("status: done"), "missing status: {stdout}");
    assert!(stdout.contains("- just done"), "missing body: {stdout}");
}

#[test]
fn resolve_verb_is_removed_and_fails() {
    let (_d, cfg) = staged_with_archived_item(
        "pwf",
        "PWF",
        "PWF-0002",
        "---\nstatus: cancelled\ntitle: dropped\nproject: pwf\n---\n\n## Goals\n- dropped\n",
    );
    pwf()
        .args(["resolve", "--show", "--id", "PWF-0002", "--config-path"])
        .arg(&cfg)
        .assert()
        .failure();
}

#[test]
fn id_input_forms_all_resolve_to_the_same_item() {
    let (_d, cfg) = staged_with_item(
        "pwf",
        "PWF",
        "PWF-0001",
        "do the thing",
        "---\nid: PWF-0001\nstatus: active\ntitle: do the thing\nproject: pwf\ncreated: 2026-06-20\n---\n\nbody\n",
    );

    let canonical = pwf()
        .args(["show", "--id", "PWF-0001", "--config-path"])
        .arg(&cfg)
        .assert()
        .success();
    let baseline = String::from_utf8(canonical.get_output().stdout.clone()).unwrap();

    for form in [
        vec!["show", "pwf-0001"],
        vec!["show", "pwf1"],
        vec!["show", "pwf", "1"],
        vec!["s", "pwf1"],
    ] {
        let out = pwf()
            .args(&form)
            .args(["--config-path"])
            .arg(&cfg)
            .assert()
            .success();
        let got = String::from_utf8(out.get_output().stdout.clone()).unwrap();
        assert_eq!(got, baseline, "form {form:?} did not resolve like --id");
    }
}

#[test]
fn show_alias_s_streams_note_markdown() {
    let (_d, cfg) = staged_with_item(
        "pwf",
        "PWF",
        "PWF-0001",
        "do the thing",
        "---\nid: PWF-0001\nstatus: active\ntitle: do the thing\nproject: pwf\ncreated: 2026-06-20\n---\n\n## Goals\n- do the thing\n",
    );
    let out = pwf()
        .args(["s", "PWF-0001", "--config-path"])
        .arg(&cfg)
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.contains("status: active"),
        "missing status: {stdout}"
    );
    assert!(stdout.contains("## Goals"), "missing body: {stdout}");
}

#[test]
fn show_alias_s_collapses_split_id_form() {
    let (_d, cfg) = staged_with_item(
        "pwf",
        "PWF",
        "PWF-0001",
        "do the thing",
        "---\nid: PWF-0001\nstatus: active\ntitle: do the thing\nproject: pwf\ncreated: 2026-06-20\n---\n\n## Goals\n- do the thing\n",
    );
    let out = pwf()
        .args(["s", "pwf", "1", "--config-path"])
        .arg(&cfg)
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("## Goals"), "missing body: {stdout}");
}

#[test]
fn show_path_prints_closed_item_path() {
    let (_d, cfg) = staged_with_archived_item(
        "pwf",
        "PWF",
        "PWF-0002",
        "---\nstatus: cancelled\ntitle: dropped\nproject: pwf\n---\n\n## Goals\n- dropped\n",
    );
    let out = pwf()
        .args(["show", "--path", "PWF-0002", "--config-path"])
        .arg(&cfg)
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.ends_with("/pwf/PWF-0002.md\n"),
        "path should point at the closed note: {stdout}"
    );
}

#[test]
fn show_finds_archived_done_item_regardless_of_status() {
    let (_d, cfg) = staged_with_archived_item(
        "pwf",
        "PWF",
        "PWF-0002",
        "---\nstatus: done\ntitle: finished thing\nproject: pwf\ncreated: 2026-06-20\n---\n\n## Goals\n- finished thing\n",
    );
    let out = pwf()
        .args(["show", "pwf-0002", "--config-path"])
        .arg(&cfg)
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("status: done"), "missing status: {stdout}");
    assert!(
        stdout.contains("- finished thing"),
        "missing body: {stdout}"
    );
}

#[test]
fn show_finds_archived_done_item_with_shorthand_id() {
    let (_d, cfg) = staged_with_archived_item(
        "pwf",
        "PWF",
        "PWF-0002",
        "---\nstatus: done\ntitle: finished thing\nproject: pwf\ncreated: 2026-06-20\n---\n\n## Goals\n- finished thing\n",
    );
    let out = pwf()
        .args(["show", "pwf-2", "--config-path"])
        .arg(&cfg)
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("status: done"), "missing status: {stdout}");
    assert!(
        stdout.contains("- finished thing"),
        "missing body: {stdout}"
    );
}

#[test]
fn show_errors_when_id_absent() {
    let (_d, cfg) = staged_with_archived_item(
        "pwf",
        "PWF",
        "PWF-0002",
        "---\nstatus: done\ntitle: t\nproject: pwf\n---\n\nbody\n",
    );
    pwf()
        .args(["show", "PWF-9999", "--config-path"])
        .arg(&cfg)
        .assert()
        .failure();
}

#[test]
fn show_preserves_raw_lowercase_id_in_not_found_error() {
    let (_d, cfg) = staged_with_archived_item(
        "pwf",
        "PWF",
        "PWF-0002",
        "---\nstatus: done\ntitle: t\nproject: pwf\n---\n\nbody\n",
    );
    let out = pwf()
        .args(["show", "pwf-9999", "--config-path"])
        .arg(&cfg)
        .assert()
        .failure();
    let stderr = String::from_utf8(out.get_output().stderr.clone()).unwrap();
    assert!(
        stderr.contains("pwf-9999"),
        "error should preserve raw lowercase id: {stderr}"
    );
    assert!(
        !stderr.contains("PWF-9999"),
        "error should not normalize the missing id: {stderr}"
    );
}

#[test]
fn show_without_id_error_names_the_command() {
    let out = pwf().arg("show").assert().failure();
    let stderr = String::from_utf8(out.get_output().stderr.clone()).unwrap();
    assert!(
        stderr.contains("--id is required for show"),
        "missing-id message should name the verb: {stderr}"
    );
}

#[test]
fn update_commits_amends_done_item_in_project_dir() {
    let (_d, cfg) = staged_with_done_item(
        "pwf",
        "PWF",
        "PWF-0003",
        "---\nstatus: done\ntitle: t\nproject: pwf\ncompleted: 2026-06-20\ncommits: \"old..HEAD\"\n---\n\nbody\n",
    );
    pwf()
        .args([
            "update",
            "--id",
            "pwf-0003",
            "--commits",
            "aaa111..bbb222",
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .success();
    let out = pwf()
        .args(["show", "--id", "PWF-0003", "--config-path"])
        .arg(&cfg)
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.contains("commits: \"aaa111..bbb222\""),
        "commits not amended: {stdout}"
    );
    assert!(!stdout.contains("old..HEAD"), "stale range left: {stdout}");
    assert!(stdout.contains("status: done"), "status changed: {stdout}");
}

#[test]
fn update_commits_amends_archived_item() {
    let (_d, cfg) = staged_with_archived_item(
        "pwf",
        "PWF",
        "PWF-0002",
        "---\nstatus: done\ntitle: t\nproject: pwf\ncompleted: 2026-06-20\n---\n\nbody\n",
    );
    pwf()
        .args([
            "update",
            "--id",
            "PWF-0002",
            "--commits",
            "c0ffee..d00d",
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .success();
    let out = pwf()
        .args(["show", "--id", "PWF-0002", "--config-path"])
        .arg(&cfg)
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.contains("commits: \"c0ffee..d00d\""),
        "commits not inserted on archived item: {stdout}"
    );
}

#[test]
fn update_commits_amends_open_item() {
    let (_d, cfg) = staged_with_item(
        "pwf",
        "PWF",
        "PWF-0001",
        "do the thing",
        "---\nid: PWF-0001\nstatus: active\ntitle: do the thing\nproject: pwf\ncreated: 2026-06-20\n---\n\n## Goals\n- do the thing\n",
    );
    pwf()
        .args([
            "update",
            "--id",
            "PWF-0001",
            "--commits",
            "1a2b..3c4d",
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .success();
    let out = pwf()
        .args(["show", "--id", "PWF-0001", "--config-path"])
        .arg(&cfg)
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.contains("commits: \"1a2b..3c4d\""),
        "commits not set on open item: {stdout}"
    );
}

#[test]
fn update_append_report_attaches_verbatim_report_to_closed_item() {
    // Closed-item reports append verbatim without rerunning body generation.
    let (_d, cfg) = staged_with_done_item(
        "pwf",
        "PWF",
        "PWF-0003",
        "---\nstatus: done\ntitle: t\nproject: pwf\ncompleted: 2026-06-20\n---\n\n## Goals\n\n- ship it\n",
    );
    let report =
        "## Outcome\n\nShipped `--append-report`.\n\n## Follow-ups\n\n- write the release notes";
    pwf()
        .args(["update", "--id", "pwf-0003", "--append-report"])
        .arg(report)
        .arg("--config-path")
        .arg(&cfg)
        .assert()
        .success()
        .stdout(contains("report appended"));
    let note = fs::read_to_string(_d.path().join("notes/pwf/PWF-0003.md")).unwrap();
    assert!(note.contains("status: done"), "status changed: {note}");
    assert!(
        note.contains("## Goals\n\n- ship it\n"),
        "body altered: {note}"
    );
    assert!(
        note.contains(
            "### Report\n\n## Outcome\n\nShipped `--append-report`.\n\n## Follow-ups\n\n- write the release notes\n"
        ),
        "report not appended verbatim: {note}"
    );
}

#[test]
fn update_append_report_rejects_whitespace_only() {
    let (_d, cfg) = staged_with_done_item(
        "pwf",
        "PWF",
        "PWF-0003",
        "---\nstatus: done\ntitle: t\nproject: pwf\ncompleted: 2026-06-20\n---\n\nbody\n",
    );
    pwf()
        .args(["update", "--id", "PWF-0003", "--append-report", "   \n\t"])
        .arg("--config-path")
        .arg(&cfg)
        .assert()
        .failure()
        .stderr(contains("--report cannot be empty"));
}

#[test]
fn update_body_edit_on_closed_item_is_rejected() {
    let (_d, cfg) = staged_with_done_item(
        "pwf",
        "PWF",
        "PWF-0003",
        "---\nstatus: done\ntitle: t\nproject: pwf\ncompleted: 2026-06-20\n---\n\nbody\n",
    );
    pwf()
        .args([
            "update",
            "--id",
            "PWF-0003",
            "--title",
            "new title",
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .failure()
        .stderr(contains("can amend closed item"));
}

#[test]
fn update_append_splices_bullets_into_an_existing_section() {
    let (_d, cfg) = staged_with_item(
        "pwf",
        "PWF",
        "PWF-0001",
        "do the thing",
        "---\nid: PWF-0001\nstatus: active\ntitle: do the thing\nproject: pwf\ncreated: 2026-06-20\n---\n\n## Goals\n- do the thing\n",
    );
    pwf()
        .args(["update", "--id", "PWF-0001", "-a", "also this"])
        .arg("--config-path")
        .arg(&cfg)
        .assert()
        .success();
    let note = fs::read_to_string(_d.path().join("notes/pwf/PWF-0001.md")).unwrap();
    assert!(
        note.contains("## Goals\n- do the thing\n- also this\n"),
        "bullet not spliced in: {note}"
    );
}

#[test]
fn update_append_creates_a_missing_section_via_lane_syntax() {
    let (_d, cfg) = staged_with_item(
        "pwf",
        "PWF",
        "PWF-0001",
        "do the thing",
        "---\nid: PWF-0001\nstatus: active\ntitle: do the thing\nproject: pwf\ncreated: 2026-06-20\n---\n\n## Goals\n- do the thing\n",
    );
    pwf()
        .args([
            "update",
            "--id",
            "PWF-0001",
            "--append",
            "another goal /c new context",
        ])
        .arg("--config-path")
        .arg(&cfg)
        .assert()
        .success();
    let note = fs::read_to_string(_d.path().join("notes/pwf/PWF-0001.md")).unwrap();
    assert!(
        note.contains("## Goals\n- do the thing\n- another goal\n\n## Context\n- new context\n"),
        "section not created: {note}"
    );
}

#[test]
fn update_append_marker_first_preserves_title_and_goals() {
    let (d, cfg) = staged_with_item(
        "pwf",
        "PWF",
        "PWF-0001",
        "do the thing",
        "---\nid: PWF-0001\nstatus: active\ntitle: do the thing\nproject: pwf\ncreated: 2026-06-20\n---\n\n## Goals\n- do the thing\n",
    );
    pwf()
        .args(["update", "--id", "PWF-0001", "--append", "/c context"])
        .arg("--config-path")
        .arg(&cfg)
        .assert()
        .success();

    let note = fs::read_to_string(d.path().join("notes/pwf/PWF-0001.md")).unwrap();
    assert!(
        note.contains("title: do the thing"),
        "title changed: {note}"
    );
    assert!(
        note.contains("## Goals\n- do the thing\n\n## Context\n- context\n"),
        "marker-first append changed unrelated content: {note}"
    );
    assert!(!note.contains("n/a"), "fallback leaked into update: {note}");
    assert!(
        !note.contains("pending work"),
        "legacy fallback leaked into update: {note}"
    );
    assert!(
        !note.contains("\n- \n"),
        "empty goal leaked into update: {note}"
    );
}

#[test]
fn update_append_rejects_whitespace_only() {
    let (_d, cfg) = staged_with_item(
        "pwf",
        "PWF",
        "PWF-0001",
        "do the thing",
        "---\nid: PWF-0001\nstatus: active\ntitle: do the thing\nproject: pwf\ncreated: 2026-06-20\n---\n\n## Goals\n- do the thing\n",
    );
    pwf()
        .args(["update", "--id", "PWF-0001", "--append", "   \n\t"])
        .arg("--config-path")
        .arg(&cfg)
        .assert()
        .failure()
        .stderr(contains("--append cannot be empty"));
}

#[test]
fn update_append_conflicts_with_prompt() {
    let (_d, cfg) = staged_with_item(
        "pwf",
        "PWF",
        "PWF-0001",
        "do the thing",
        "---\nid: PWF-0001\nstatus: active\ntitle: do the thing\nproject: pwf\ncreated: 2026-06-20\n---\n\n## Goals\n- do the thing\n",
    );
    pwf()
        .args([
            "update", "--id", "PWF-0001", "--prompt", "x", "--append", "y",
        ])
        .arg("--config-path")
        .arg(&cfg)
        .assert()
        .failure()
        .stderr(contains("cannot be used with"));
}

#[test]
fn update_append_on_closed_item_is_rejected() {
    let (_d, cfg) = staged_with_done_item(
        "pwf",
        "PWF",
        "PWF-0003",
        "---\nstatus: done\ntitle: t\nproject: pwf\ncompleted: 2026-06-20\n---\n\nbody\n",
    );
    pwf()
        .args(["update", "--id", "PWF-0003", "--append", "more work"])
        .arg("--config-path")
        .arg(&cfg)
        .assert()
        .failure()
        .stderr(contains("can amend closed item"));
}

/// Stages a launchable item and a recording `zellij` stub on the child process PATH.
/// Returns the config, child PATH, and argv log.
#[cfg(unix)]
fn stage_session_with_zellij_stub(
    dir: &TempDir,
) -> (std::path::PathBuf, String, std::path::PathBuf) {
    use std::os::unix::fs::PermissionsExt;

    // Session preflight requires the mapped repository to exist.
    let notes = dir.path().join("notes");
    let proj = notes.join("pwf");
    let repo = dir.path().join("repo");
    fs::create_dir_all(&proj).unwrap();
    fs::create_dir_all(&repo).unwrap();
    fs::write(
        proj.join("PWF-0001.md"),
        "---\nid: PWF-0001\nstatus: active\ntitle: do the thing\nproject: pwf\ncreated: 2026-06-20\n---\n\n## Goals\n- do the thing\n",
    )
    .unwrap();
    fs::write(proj.join("pwf.md"), "- [ ] [[PWF-0001|do the thing]]\n").unwrap();
    let cfg = dir.path().join("cfg.json");
    fs::write(
        &cfg,
        format!(
            r#"{{ "notesDir": {:?}, "projects": {{ "pwf": {:?} }}, "prefixes": {{ "pwf": "PWF" }} }}"#,
            notes.to_string_lossy(),
            repo.to_string_lossy()
        ),
    )
    .unwrap();
    migrate_fixture(&cfg);

    // Install the recording fixture as `zellij` on the child PATH.
    let bin = dir.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    let stub = bin.join("zellij");
    fs::copy("tests/fixtures/zellij-stub.sh", &stub).unwrap();
    fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();

    let log = dir.path().join("argv.log");
    let path = format!(
        "{}:{}",
        bin.to_string_lossy(),
        std::env::var("PATH").unwrap_or_default()
    );
    (cfg, path, log)
}

#[test]
#[cfg(unix)]
fn session_ok_outputs_thread_title() {
    let dir = TempDir::new().unwrap();
    let (cfg, path, log) = stage_session_with_zellij_stub(&dir);

    pwf()
        .args(["session", "--id", "PWF-0001", "--yes", "--config-path"])
        .arg(&cfg)
        .env("PATH", path)
        .env("ZELLIJ_STUB_LOG", &log)
        .assert()
        .success()
        .stdout(contains("dispatched"));

    let argv = fs::read_to_string(&log).unwrap();
    assert!(
        argv.contains("PWF-0001 - do the thing"),
        "thread title not in captured zellij argv: {argv}"
    );
}

#[test]
#[cfg(unix)]
fn session_worktree_flag_injects_instruction_into_argv() {
    let dir = TempDir::new().unwrap();
    let (cfg, path, log) = stage_session_with_zellij_stub(&dir);

    pwf()
        .args([
            "session",
            "--id",
            "PWF-0001",
            "--yes",
            "-w",
            "--config-path",
        ])
        .arg(&cfg)
        .env("PATH", path)
        .env("ZELLIJ_STUB_LOG", &log)
        .assert()
        .success()
        .stdout(contains("dispatched"));

    let argv = fs::read_to_string(&log).unwrap();
    assert!(
        argv.contains("git-worktrees skill") && argv.contains("named `PWF-0001`"),
        "worktree instruction not in captured zellij argv: {argv}"
    );
}

#[test]
#[cfg(unix)]
fn session_without_worktree_flag_omits_instruction() {
    let dir = TempDir::new().unwrap();
    let (cfg, path, log) = stage_session_with_zellij_stub(&dir);

    pwf()
        .args(["session", "--id", "PWF-0001", "--yes", "--config-path"])
        .arg(&cfg)
        .env("PATH", path)
        .env("ZELLIJ_STUB_LOG", &log)
        .assert()
        .success();

    let argv = fs::read_to_string(&log).unwrap();
    assert!(
        !argv.contains("worktree"),
        "worktree step leaked without -w: {argv}"
    );
}

#[test]
#[cfg(unix)]
fn session_with_effort_passes_model_flag_to_claude() {
    let dir = TempDir::new().unwrap();
    let (cfg, path, log) = stage_session_with_zellij_stub(&dir);
    // Add effort metadata without duplicating the session fixture.
    let notes = dir.path().join("notes");
    fs::write(
        notes.join("pwf").join("PWF-0001.md"),
        "---\nid: PWF-0001\nstatus: active\ntitle: do the thing\nproject: pwf\ncreated: 2026-06-20\neffort: 4\n---\n\n## Goals\n- do the thing\n",
    )
    .unwrap();
    let tiers = dir.path().join("model-tiers.toml");
    fs::write(&tiers, "[tiers.4]\nclaude_model = \"opus\"\n").unwrap();

    pwf()
        .args(["session", "--id", "PWF-0001", "--yes", "--config-path"])
        .arg(&cfg)
        .env("PATH", path)
        .env("ZELLIJ_STUB_LOG", &log)
        .env("PWF_MODEL_TIERS", &tiers)
        .assert()
        .success();

    let argv = fs::read_to_string(&log).unwrap();
    assert!(argv.contains("--model"), "no --model in argv: {argv}");
    assert!(argv.contains("opus"), "model value missing: {argv}");
}

#[test]
#[cfg(unix)]
fn session_with_effort_and_empty_claude_model_omits_model_flag() {
    // An empty tier mapping delegates model selection to Claude.
    let dir = TempDir::new().unwrap();
    let (cfg, path, log) = stage_session_with_zellij_stub(&dir);
    let notes = dir.path().join("notes");
    fs::write(
        notes.join("pwf").join("PWF-0001.md"),
        "---\nid: PWF-0001\nstatus: active\ntitle: do the thing\nproject: pwf\ncreated: 2026-06-20\neffort: 4\n---\n\n## Goals\n- do the thing\n",
    )
    .unwrap();
    let tiers = dir.path().join("model-tiers.toml");
    fs::write(&tiers, "[tiers.4]\nclaude_model = \"\"\n").unwrap();

    pwf()
        .args(["session", "--id", "PWF-0001", "--yes", "--config-path"])
        .arg(&cfg)
        .env("PATH", path)
        .env("ZELLIJ_STUB_LOG", &log)
        .env("PWF_MODEL_TIERS", &tiers)
        .assert()
        .success();

    let argv = fs::read_to_string(&log).unwrap();
    assert!(
        !argv.contains("--model"),
        "unexpected --model in argv: {argv}"
    );
}

#[test]
#[cfg(unix)]
fn session_with_explicit_model_flag_forwards_it_verbatim() {
    // Explicit models bypass effort-tier lookup and forward verbatim.
    let dir = TempDir::new().unwrap();
    let (cfg, path, log) = stage_session_with_zellij_stub(&dir);

    pwf()
        .args([
            "session",
            "--id",
            "PWF-0001",
            "--yes",
            "--model",
            "fable",
            "--config-path",
        ])
        .arg(&cfg)
        .env("PATH", path)
        .env("ZELLIJ_STUB_LOG", &log)
        .assert()
        .success();

    let argv = fs::read_to_string(&log).unwrap();
    assert!(argv.contains("--model"), "no --model in argv: {argv}");
    assert!(argv.contains("fable"), "model value missing: {argv}");
}

#[test]
#[cfg(unix)]
fn session_with_explicit_model_flag_wins_over_effort_tier() {
    let dir = TempDir::new().unwrap();
    let (cfg, path, log) = stage_session_with_zellij_stub(&dir);
    let notes = dir.path().join("notes");
    fs::write(
        notes.join("pwf").join("PWF-0001.md"),
        "---\nid: PWF-0001\nstatus: active\ntitle: do the thing\nproject: pwf\ncreated: 2026-06-20\neffort: 4\n---\n\n## Goals\n- do the thing\n",
    )
    .unwrap();
    let tiers = dir.path().join("model-tiers.toml");
    fs::write(&tiers, "[tiers.4]\nclaude_model = \"opus\"\n").unwrap();

    pwf()
        .args([
            "session",
            "--id",
            "PWF-0001",
            "--yes",
            "--model",
            "fable",
            "--config-path",
        ])
        .arg(&cfg)
        .env("PATH", path)
        .env("ZELLIJ_STUB_LOG", &log)
        .env("PWF_MODEL_TIERS", &tiers)
        .assert()
        .success();

    let argv = fs::read_to_string(&log).unwrap();
    assert!(argv.contains("fable"), "override should win: {argv}");
    assert!(
        !argv.contains("opus"),
        "tier model should be shadowed: {argv}"
    );
}

#[test]
#[cfg(unix)]
fn session_with_explicit_model_flag_survives_broken_tiers_config() {
    let dir = TempDir::new().unwrap();
    let (cfg, path, log) = stage_session_with_zellij_stub(&dir);
    let notes = dir.path().join("notes");
    fs::write(
        notes.join("pwf").join("PWF-0001.md"),
        "---\nid: PWF-0001\nstatus: active\ntitle: do the thing\nproject: pwf\ncreated: 2026-06-20\neffort: 4\n---\n\n## Goals\n- do the thing\n",
    )
    .unwrap();
    let missing_tiers = dir.path().join("does-not-exist.toml");

    pwf()
        .args([
            "session",
            "--id",
            "PWF-0001",
            "--yes",
            "--model",
            "fable",
            "--config-path",
        ])
        .arg(&cfg)
        .env("PATH", path)
        .env("ZELLIJ_STUB_LOG", &log)
        .env("PWF_MODEL_TIERS", &missing_tiers)
        .assert()
        .success();

    let argv = fs::read_to_string(&log).unwrap();
    assert!(argv.contains("fable"), "override should win: {argv}");
}

#[test]
#[cfg(unix)]
fn session_with_effort_and_broken_tiers_config_fails_before_dispatch() {
    let dir = TempDir::new().unwrap();
    let (cfg, path, log) = stage_session_with_zellij_stub(&dir);
    let notes = dir.path().join("notes");
    fs::write(
        notes.join("pwf").join("PWF-0001.md"),
        "---\nid: PWF-0001\nstatus: active\ntitle: do the thing\nproject: pwf\ncreated: 2026-06-20\neffort: 4\n---\n\n## Goals\n- do the thing\n",
    )
    .unwrap();
    let missing_tiers = dir.path().join("does-not-exist.toml");

    pwf()
        .args(["session", "--id", "PWF-0001", "--yes", "--config-path"])
        .arg(&cfg)
        .env("PATH", path)
        .env("ZELLIJ_STUB_LOG", &log)
        .env("PWF_MODEL_TIERS", &missing_tiers)
        .assert()
        .failure();

    assert!(!log.exists() || fs::read_to_string(&log).unwrap().is_empty());
}

#[test]
#[cfg(unix)]
fn session_codex_agent_emits_codex_argv() {
    // Codex thread naming uses the hidden app-server shim because Codex has no `--name` flag.
    let dir = TempDir::new().unwrap();
    let (cfg, path, log) = stage_session_with_zellij_stub(&dir);

    pwf()
        .args([
            "session",
            "--id",
            "PWF-0001",
            "--agent",
            "codex",
            "--yes",
            "--config-path",
        ])
        .arg(&cfg)
        .env("PATH", path)
        .env("ZELLIJ_STUB_LOG", &log)
        .assert()
        .success()
        .stdout(contains("dispatched"));

    let argv = fs::read_to_string(&log).unwrap();
    assert!(
        argv.contains("__codex-thread-title"),
        "codex title shim not captured: {argv}"
    );
    assert!(
        argv.contains("PWF-0001 - do the thing"),
        "codex title shim did not receive get_thread_title text: {argv}"
    );
    // Match the Codex segment because zellij also contributes a `--name` argument.
    assert!(
        argv.contains("-- codex --"),
        "codex argv tail not captured: {argv}"
    );
    assert!(
        !argv.contains("codex --name"),
        "codex must not get a --name flag: {argv}"
    );
}

#[test]
#[cfg(unix)]
fn session_append_extends_the_note_before_dispatch() {
    // Session appends to the note while the launch argv remains a thin pointer.
    let dir = TempDir::new().unwrap();
    let (cfg, path, log) = stage_session_with_zellij_stub(&dir);

    pwf()
        .args([
            "session",
            "--id",
            "PWF-0001",
            "--yes",
            "--append",
            "one more thing in the moment",
            "--config-path",
        ])
        .arg(&cfg)
        .env("PATH", path)
        .env("ZELLIJ_STUB_LOG", &log)
        .assert()
        .success()
        .stdout(contains("dispatched"));

    let note = fs::read_to_string(dir.path().join("notes/pwf/PWF-0001.md")).unwrap();
    assert!(
        note.contains("- one more thing in the moment"),
        "append did not extend the note body: {note}"
    );

    let argv = fs::read_to_string(&log).unwrap();
    assert!(
        argv.contains("do PWF-0001"),
        "dispatched prompt did not carry the thin pointer: {argv}"
    );
    assert!(
        !argv.contains("one more thing in the moment"),
        "the note body must not be inlined into the dispatched prompt: {argv}"
    );
}

#[test]
#[cfg(unix)]
fn session_dispatches_a_thin_pointer_not_the_note_body() {
    // The agent resolves the item, so dispatch includes its identity but not its body.
    let dir = TempDir::new().unwrap();
    let (cfg, path, log) = stage_session_with_zellij_stub(&dir);

    pwf()
        .args(["session", "--id", "PWF-0001", "--yes", "--config-path"])
        .arg(&cfg)
        .env("PATH", path)
        .env("ZELLIJ_STUB_LOG", &log)
        .assert()
        .success()
        .stdout(contains("dispatched"));

    let argv = fs::read_to_string(&log).unwrap();
    assert!(
        argv.contains("Pending-work ID: PWF-0001") && argv.contains("Project: pwf"),
        "dispatched prompt lost its id/project headers: {argv}"
    );
    assert!(
        argv.contains("do PWF-0001"),
        "dispatched prompt is missing the thin pointer: {argv}"
    );
    assert!(
        !argv.contains("## Goals"),
        "the note body must not be inlined into the dispatched prompt: {argv}"
    );
}

#[test]
#[cfg(unix)]
fn session_append_short_flag_extends_body_not_the_agent() {
    let dir = TempDir::new().unwrap();
    let (cfg, path, log) = stage_session_with_zellij_stub(&dir);

    pwf()
        .args(["session", "--id", "PWF-0001", "--yes", "-a", "extra note"])
        .arg("--config-path")
        .arg(&cfg)
        .env("PATH", path)
        .env("ZELLIJ_STUB_LOG", &log)
        .assert()
        .success()
        .stdout(contains("dispatched"));

    let note = fs::read_to_string(dir.path().join("notes/pwf/PWF-0001.md")).unwrap();
    assert!(
        note.contains("- extra note"),
        "-a did not splice into the body: {note}"
    );

    let argv = fs::read_to_string(&log).unwrap();
    assert!(
        argv.contains("claude"),
        "default agent must stay claude: {argv}"
    );
    assert!(
        !argv.contains("codex"),
        "-a must not select codex as an agent: {argv}"
    );
}

#[test]
fn session_append_rejects_whitespace_only_before_any_dispatch() {
    let (dir, cfg) = staged();
    let note_path = dir.path().join("notes/glep-shimeji/GLP-0001.md");
    let before = fs::read_to_string(&note_path).unwrap();

    pwf()
        .args([
            "session",
            "--id",
            "GLP-0001",
            "--yes",
            "--append",
            "   \n\t",
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .failure()
        .stderr(contains("--append cannot be empty"));

    let after = fs::read_to_string(&note_path).unwrap();
    assert_eq!(before, after, "note must be untouched on a rejected append");
}

#[test]
fn session_rejects_unknown_agent() {
    pwf()
        .args([
            "session",
            "PWF-0001",
            "--agent",
            "bogus",
            "--config-path",
            "x",
        ])
        .assert()
        .failure()
        .stderr(contains("invalid value 'bogus'"));
}

#[test]
fn verify_codex_agent_reports_codex() {
    // The binary-only Codex command is stable even when Codex is absent from the host PATH.
    pwf()
        .args(["verify", "--agent", "codex"])
        .assert()
        .success()
        .stdout(contains("codex:"))
        .stdout(contains("command: codex"));
}

#[test]
fn e2e_verify_reports_resolved_model_for_effort_tagged_item() {
    let (d, cfg) = staged();
    pwf()
        .args([
            "update",
            "--id",
            "GLP-0001",
            "--effort",
            "1",
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .success();
    let tiers = d.path().join("model-tiers.toml");
    fs::write(&tiers, "[tiers.1]\nclaude_model = \"sonnet\"\n").unwrap();

    pwf()
        .args(["verify", "--id", "GLP-0001", "--config-path"])
        .arg(&cfg)
        .env("PWF_MODEL_TIERS", &tiers)
        .assert()
        .success()
        .stdout(contains("--model"))
        .stdout(contains("sonnet"));
}

#[test]
fn e2e_verify_omits_model_for_empty_claude_model_tier() {
    let (d, cfg) = staged();
    pwf()
        .args([
            "update",
            "--id",
            "GLP-0001",
            "--effort",
            "1",
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .success();
    let tiers = d.path().join("model-tiers.toml");
    fs::write(&tiers, "[tiers.1]\nclaude_model = \"\"\n").unwrap();

    let out = pwf()
        .args(["verify", "--id", "GLP-0001", "--config-path"])
        .arg(&cfg)
        .env("PWF_MODEL_TIERS", &tiers)
        .assert()
        .success()
        .stdout(contains("launchable: yes"))
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(out).unwrap();
    assert!(
        !stdout.contains("--model"),
        "unexpected --model in verify output: {stdout}"
    );
}

#[test]
fn e2e_verify_with_explicit_model_flag_wins_over_effort_tier() {
    let (d, cfg) = staged();
    pwf()
        .args([
            "update",
            "--id",
            "GLP-0001",
            "--effort",
            "1",
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .success();
    let tiers = d.path().join("model-tiers.toml");
    fs::write(&tiers, "[tiers.1]\nclaude_model = \"sonnet\"\n").unwrap();

    pwf()
        .args([
            "verify",
            "--id",
            "GLP-0001",
            "--model",
            "fable",
            "--config-path",
        ])
        .arg(&cfg)
        .env("PWF_MODEL_TIERS", &tiers)
        .assert()
        .success()
        .stdout(contains("--model"))
        .stdout(contains("fable"))
        .stdout(contains("sonnet").not());
}

#[test]
fn e2e_verify_fails_on_broken_model_tiers_for_effort_tagged_item() {
    let (d, cfg) = staged();
    pwf()
        .args([
            "update",
            "--id",
            "GLP-0001",
            "--effort",
            "1",
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .success();
    let missing_tiers = d.path().join("does-not-exist.toml");

    pwf()
        .args(["verify", "--id", "GLP-0001", "--config-path"])
        .arg(&cfg)
        .env("PWF_MODEL_TIERS", &missing_tiers)
        .assert()
        .success() // Probe failures are reported in Markdown.
        .stdout(contains("\u{2014} fail"))
        .stdout(contains("launchable: no"));
}

#[test]
fn session_missing_id_errors() {
    pwf()
        .arg("session")
        .assert()
        .failure()
        .stderr(contains("--id is required for session"));
}

#[test]
fn session_unknown_id_errors_not_found() {
    let (_dir, cfg) = staged();
    pwf()
        .args(["session", "GLP-9999", "--config-path"])
        .arg(&cfg)
        .assert()
        .failure()
        .stderr(contains("not found"));
}

#[test]
fn session_accepts_yes_flag() {
    let (_dir, cfg) = staged();
    pwf()
        .args(["session", "GLP-9999", "--yes", "--config-path"])
        .arg(&cfg)
        .assert()
        .failure()
        .stderr(contains("not found"));
}

#[test]
fn session_accepts_inline_short_flag() {
    let (_dir, cfg) = staged();
    pwf()
        .args(["session", "GLP-9999", "-i", "--config-path"])
        .arg(&cfg)
        .assert()
        .failure()
        .stderr(contains("not found"));
}

#[test]
fn session_accepts_inline_long_flag() {
    let (_dir, cfg) = staged();
    pwf()
        .args(["session", "GLP-9999", "--inline", "--config-path"])
        .arg(&cfg)
        .assert()
        .failure()
        .stderr(contains("not found"));
}

#[test]
fn session_accepts_worktree_flags() {
    let (_dir, cfg) = staged();
    for flag in ["-w", "--worktree"] {
        pwf()
            .args(["session", "GLP-9999", flag, "--config-path"])
            .arg(&cfg)
            .assert()
            .failure()
            .stderr(contains("not found"));
    }
}

#[test]
fn retired_launch_verb_treated_as_unknown_project() {
    // Retired launch verbs fall through to project routing.
    pwf()
        .args(["launch", "--id", "GLP-0001"])
        .assert()
        .failure()
        .stderr(contains("Unknown managed project identifier"));
}

#[test]
fn retired_launch_claude_verb_treated_as_unknown_project() {
    pwf()
        .args(["launch-claude", "--id", "GLP-0001"])
        .assert()
        .failure()
        .stderr(contains("Unknown managed project identifier"));
}

#[test]
fn e2e_remove_resolves_descriptive_filename_by_frontmatter_id() {
    let (d, cfg) = staged();
    let project = d.path().join("notes/glep-shimeji");
    fs::rename(
        project.join("GLP-0001.md"),
        project.join("descriptive-name.md"),
    )
    .unwrap();

    pwf()
        .args(["remove", "--id", "GLP-0001", "--yes", "--config-path"])
        .arg(&cfg)
        .assert()
        .success();

    assert!(!project.join("descriptive-name.md").exists());
}

#[test]
fn note_add_list_update_remove_preserves_tasks_and_header() {
    let (dir, cfg) = staged_with_item(
        "pwf",
        "PWF",
        "PWF-0001",
        "real task",
        "---\nstatus: active\ntitle: real task\nproject: pwf\ncreated: 2026-01-01\n---\n\nbody\n",
    );

    pwf()
        .args(["note", "add", "pwf", "remember the milk", "--config-path"])
        .arg(&cfg)
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "PWF-NOTE-0001 :: remember the milk",
        ));

    let index = fs::read_to_string(dir.path().join("notes/pwf/pwf.md")).unwrap();
    assert!(
        index.contains("- [ ] [[PWF-0001|real task]]"),
        "task clobbered: {index}"
    );
    assert!(index.contains("### Notes"), "notes header missing: {index}");
    assert!(
        index.contains("- [[PWF-NOTE-0001]]"),
        "note link missing: {index}"
    );

    // The project token resolves case-insensitively by name or id code.
    pwf()
        .args([
            "note",
            "update",
            "PWF",
            "1",
            "remember oat milk",
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "Updated PWF-NOTE-0001 :: remember oat milk",
        ));

    // A bare project lists (implicit `ls`).
    pwf()
        .args(["note", "pwf", "--config-path"])
        .arg(&cfg)
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "PWF-NOTE-0001 :: remember oat milk",
        ));

    let updated_index = fs::read_to_string(dir.path().join("notes/pwf/pwf.md")).unwrap();
    assert_eq!(
        updated_index.matches("- [[PWF-NOTE-0001]]").count(),
        1,
        "update duplicated note link: {updated_index}"
    );

    pwf()
        .args(["note", "remove", "pwf", "1", "--config-path"])
        .arg(&cfg)
        .assert()
        .success();

    let after = fs::read_to_string(dir.path().join("notes/pwf/pwf.md")).unwrap();
    assert!(
        after.contains("- [ ] [[PWF-0001|real task]]"),
        "task lost on remove: {after}"
    );
    assert!(
        !after.contains("- [[PWF-NOTE-0001]]"),
        "note line lingered: {after}"
    );
    assert!(
        after.contains("### Notes"),
        "notes header stripped on remove: {after}"
    );
}

#[test]
fn e2e_add_done_reopen_handoff_tag_round_trip_never_touches_git() {
    // The fixture omits `.git`; the complete handoff lifecycle must neither require nor create it.
    let (d, cfg) = staged_for_handoff_mirror_roundtrip();
    let repo = d.path().join("repo");
    let handoff_dir = repo.join("docs/handoffs");
    let handoff_path = handoff_dir.join("2026-01-01-mirror-round-trip.md");
    assert!(
        !repo.join(".git").exists(),
        "fixture must start without .git"
    );

    pwf()
        .args([
            "add",
            "glep-shimeji",
            "mirror round trip",
            "--tag",
            "handoff",
            "--title",
            "mirror round trip",
            "--date",
            "2026-01-01",
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .success();
    let scaffolded = fs::read_to_string(&handoff_path).unwrap_or_else(|e| {
        panic!(
            "handoff scaffold missing at {}: {e}",
            handoff_path.display()
        )
    });
    assert!(scaffolded.contains("status: active"), "got: {scaffolded}");
    assert!(scaffolded.contains("pw: GLP-0001"), "got: {scaffolded}");
    assert!(
        !repo.join(".git").exists(),
        "add must not create/touch .git"
    );

    pwf()
        .args([
            "done",
            "--id",
            "GLP-0001",
            "--report",
            "x",
            "--commits",
            "a..b",
            "--date",
            "2026-01-02",
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .success();
    assert!(
        !handoff_path.exists(),
        "handoff should be moved out of the active dir once done"
    );
    let archived_path = handoff_dir.join("archived").join(
        handoff_path
            .file_name()
            .expect("handoff path must have a file name"),
    );
    let archived = fs::read_to_string(&archived_path).unwrap();
    assert!(archived.contains("status: done"), "got: {archived}");
    assert!(
        !repo.join(".git").exists(),
        "done must not create/touch .git"
    );

    pwf()
        .args(["reopen", "--id", "GLP-0001", "--config-path"])
        .arg(&cfg)
        .assert()
        .success();
    assert!(
        !archived_path.exists(),
        "reopen should move the handoff back out of archived/"
    );
    let restored = fs::read_to_string(&handoff_path).unwrap();
    assert!(restored.contains("status: active"), "got: {restored}");
    assert!(!restored.contains("completed:"), "got: {restored}");
    assert!(
        !repo.join(".git").exists(),
        "reopen must not create/touch .git"
    );
}
