//! Behavior-level checks of the built binary (clap surface), per the
//! rust-cli-tooling testing matrix: exit codes, that the retired legacy flag
//! surface now errors, and canonical-only flags.

use std::fs;

use assert_cmd::Command;
use predicates::{prelude::PredicateBooleanExt, str::contains};
use tempfile::TempDir;

fn pwf() -> Command {
    Command::cargo_bin("pwf").unwrap()
}

/// A staged notes dir with one open item + its config.json.
fn staged() -> (TempDir, std::path::PathBuf) {
    let dir = TempDir::new().unwrap();
    let notes = dir.path().join("notes");
    let proj = notes.join("glep-shimeji");
    fs::create_dir_all(&proj).unwrap();
    fs::write(
        proj.join("GLP-0001.md"),
        "---\nstatus: active\ntitle: tray gui\nproject: glep-shimeji\ncreated: 2026-01-01\n---\n\nadd toggle\n",
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
    (dir, cfg)
}

/// Like `staged()`, but with a second open item `GLP-0002` so `--prereq` can point at
/// a distinct valid id. Kept separate from `staged()` to avoid skewing item counts in
/// the cap/list tests that assume a single seeded item.
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
    (dir, cfg)
}

/// Like `staged()`, but maps glep-shimeji at a real repo dir holding one handoff
/// note — needed by `--continue-handoff`, which reads `<repo>/docs/handoffs/`.
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
    (dir, cfg)
}

/// Read the glep-shimeji index under a staged notes dir (TempDir root).
fn read_index(dir: &TempDir) -> String {
    fs::read_to_string(dir.path().join("notes/glep-shimeji/glep-shimeji.md")).unwrap()
}

/// Like `staged()`, but seeds `count` open items (GLP-0001..=GLP-{count}) so the
/// default list cap (10) actually truncates (PWF-0020).
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
    (dir, cfg)
}

#[test]
fn add_positional_quoted_prompt_creates_item() {
    let (d, cfg) = staged();
    pwf()
        .args(["add", "glep-shimeji", "x y z", "--config-path"])
        .arg(&cfg)
        .assert()
        .success();
    // staged() already holds GLP-0001, so the new item is GLP-0002.
    assert!(d.path().join("notes/glep-shimeji/GLP-0002.md").exists());
    assert!(read_index(&d).contains("[[GLP-0002]]"), "index not updated");
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
        stdout.starts_with("ADDED PWF TASK [GLP-0001]"),
        "got: {stdout}"
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
    // The footgun is dead: no item was written.
    assert!(!d.path().join("notes/glep-shimeji/GLP-0002.md").exists());
}

#[test]
fn deprecated_pw_prefix_warns_and_fails() {
    let (_d, cfg) = staged();
    pwf()
        .args(["pw", "list", "--config-path"])
        .arg(&cfg)
        .assert()
        .failure()
        .stderr(contains("warning: `pwf pw ...` is deprecated"))
        .stderr(contains("use `pwf list`"));
}

#[test]
fn pending_work_alias_warns_and_fails() {
    let (_d, cfg) = staged();
    pwf()
        .args(["pending-work", "list", "--config-path"])
        .arg(&cfg)
        .assert()
        .failure()
        .stderr(contains("warning: `pwf pending-work ...` is deprecated"))
        .stderr(contains("use `pwf list`"));
}

#[test]
fn deprecated_pw_prefix_fails_for_action_usage_without_writing() {
    let (d, cfg) = staged();
    pwf()
        .args(["pw", "add", "glep-shimeji", "legacy add", "--config-path"])
        .arg(&cfg)
        .assert()
        .failure()
        .stderr(contains("warning: `pwf pw ...` is deprecated"))
        .stderr(contains("use `pwf add`"));
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
fn help_and_version_exit_zero() {
    pwf()
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("pw"))
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
fn retired_legacy_flag_surface_errors() {
    // The PowerShell-style surface was removed in v2.1: `-Action`/`-ConfigPath`
    // no longer parse. `-Action list` is treated as router words, so the run
    // fails (unknown project) rather than silently listing.
    let (_d, cfg) = staged();
    pwf()
        .args(["-Action", "list", "-ConfigPath"])
        .arg(&cfg)
        .assert()
        .failure();
}

#[test]
fn canonical_only_prereq_flag_works() {
    // `--prereq` is a canonical CLI-only surface; cover it through the binary.
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

// ── PWF-0031: post-port e2e coverage (assert file CONTENTS, not just exit) ──────
// The in-process tests in tests/pending_work.rs mirror these; here we run the binary.

/// Read a staged item file under the glep-shimeji project.
fn read_item(dir: &TempDir, id: &str) -> String {
    fs::read_to_string(dir.path().join(format!("notes/glep-shimeji/{id}.md"))).unwrap()
}

/// Extract the `title: ` frontmatter value from an item file.
fn title_of(item: &str) -> &str {
    item.lines()
        .find_map(|l| l.strip_prefix("title: "))
        .expect("title frontmatter present")
}

// 1. `update` verb — zero e2e coverage today (mirrors pending_work.rs:862-993).

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
    // Body is the Goals template, one bullet per slash lane.
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
    // Frontmatter preserved untouched.
    assert!(item.contains("status: active"));
    assert!(item.contains("title: tray gui"));
    assert!(item.contains("project: glep-shimeji"));
    assert!(item.contains("created: 2026-01-01"));
    // Atomic write leaves no .bak clutter.
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

// 2. Rich prompt lanes via the binary.

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
    // Title is the lead clause, lowercased, cut at the first lane marker.
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

// 3. Title cap end-to-end (mirrors pending_work.rs:134-189).

#[test]
fn e2e_add_caps_long_title_without_ampersand() {
    let (d, cfg) = staged();
    // >120 chars, no `&`; the tail "smallest first" must be dropped from the title.
    let long = "Continue the PowerShell to Rust port into the cfgtool CLI using the shipped gaming domain as the template porting smallest first";
    pwf()
        .args(["add", "glep-shimeji", long, "--config-path"])
        .arg(&cfg)
        .assert()
        .success();
    let item = read_item(&d, "GLP-0002");
    let title = title_of(&item);
    // Cap appends `…`, so the bound is MAX_TITLE_CHARS + 1 = 81, not exactly 80.
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
    // The dropped tail still lives in the Goals body.
    assert!(
        item.contains("smallest first"),
        "body keeps full prompt: {item}"
    );
}

// 4. Lowercase normalization end-to-end (guards commit 063304b) — add + update.

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

// 5. Done-queue rotation via `check` (mirrors pending_work.rs:720-833).

#[test]
fn e2e_check_rotates_done_queue_past_general_cap() {
    // staged_many(7): seven open items. Check all seven; the General cap is 6, so
    // the oldest done entry (GLP-0001) is evicted past the cap.
    let (d, cfg) = staged_many(7);
    for n in 1..=7 {
        let id = format!("GLP-{n:04}");
        pwf()
            .args([
                "check",
                "--id",
                &id,
                "--date",
                "2026-01-01",
                "--config-path",
            ])
            .arg(&cfg)
            .assert()
            .success();
    }
    let index = read_index(&d);
    // The most-recently checked item is marked done in place with the date.
    assert!(
        index.contains("- [x] [[GLP-0007]] ✅ 2026-01-01"),
        "checked item not marked in place: {index}"
    );
    // Only `cap` (6) done entries remain; the oldest was evicted.
    assert_eq!(
        index.matches("- [x]").count(),
        6,
        "cap not enforced: {index}"
    );
    assert!(!index.contains("GLP-0001"), "oldest not evicted: {index}");
    // Evicted note archived, not deleted; atomic writes leave no .bak.
    assert!(
        d.path()
            .join("notes/glep-shimeji/_archive/GLP-0001.md")
            .exists()
    );
    assert!(
        !d.path()
            .join("notes/glep-shimeji/glep-shimeji.md.bak")
            .exists()
    );
}

// 6. Prereq frontmatter content end-to-end (mirrors pending_work.rs:256-343).

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

// 6b. `update --prereq`/`--clear-prereq` (PWF-0042) — append/clear semantics.

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
    // Seed GLP-0002 with an existing prereq, then append an overlapping set.
    let proj = d.path().join("notes/glep-shimeji");
    fs::write(
        proj.join("GLP-0002.md"),
        "---\nstatus: active\ntitle: second\nproject: glep-shimeji\ncreated: 2026-01-02\nprereq: \"[[GLP-0001]]\"\n---\n\ndo more\n",
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
    // GLP-0001 already present is the only valid append target; it must not duplicate.
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
        "---\nstatus: active\ntitle: second\nproject: glep-shimeji\ncreated: 2026-01-02\nprereq: \"[[GLP-0001]]\"\n---\n\ndo more\n",
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
    // Other frontmatter intact.
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
    // clap rejects the mutually-exclusive flags before the engine runs.
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

// 7. Default section placement end-to-end (mirrors pending_work.rs:192-253).

#[test]
fn e2e_add_default_section_lands_before_any_header() {
    let (d, cfg) = staged();
    // Seed a `## ` header so the placement assertion below actually runs: the
    // default add must land in the top-level General region, above this section.
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

// 8. PWF-0017: commit-range provenance on `check` + the explicit `--review` task.

#[test]
fn e2e_check_normalizes_mixed_case_id() {
    let (d, cfg) = staged();
    pwf()
        .args([
            "check",
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
fn e2e_check_commits_writes_provenance_frontmatter() {
    let (d, cfg) = staged();
    pwf()
        .args([
            "check",
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
fn e2e_check_commits_repeated_and_comma_join_and_dedup() {
    let (d, cfg) = staged();
    pwf()
        .args([
            "check",
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

    // Comma form yields the identical joined value.
    let (d2, cfg2) = staged();
    pwf()
        .args([
            "check",
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
fn e2e_check_without_commits_writes_no_commits_line() {
    // Byte-identical default path: no `--commits` ⇒ no `commits:` frontmatter.
    let (d, cfg) = staged();
    pwf()
        .args([
            "check",
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
        "default check path leaked a commits line"
    );
}

#[test]
fn e2e_check_review_spawns_human_task_scoped_to_range() {
    let (d, cfg) = staged();
    pwf()
        .args([
            "check",
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
    // The checked item still gains the commits provenance.
    assert!(
        read_item(&d, "GLP-0001").contains("commits: \"a..b\""),
        "checked item missing commits"
    );
    // A new `## Human` review task is spawned (GLP-0002) with the prepped commands.
    let index = read_index(&d);
    assert!(index.contains("## Human"), "no Human section: {index}");
    let spawned = read_item(&d, "GLP-0002");
    assert!(
        spawned.contains("git-tools diff a..b"),
        "review task missing scoped diff: {spawned}"
    );
    assert!(
        spawned.contains("git-tools diff-subrepos"),
        "review task missing subrepos diff: {spawned}"
    );
}

#[test]
fn e2e_check_review_appends_review_task_as_text() {
    // check is now text-only (PWF-0059).
    let (_d, cfg) = staged();
    let out = pwf()
        .args([
            "check",
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
        stdout.starts_with("Checked GLP-0001"),
        "expected text output: {stdout}"
    );
    assert!(
        stdout.contains("ADDED PWF TASK [GLP-0002]"),
        "review task appended as text: {stdout}"
    );
}

#[test]
fn e2e_check_review_without_commits_uses_bare_diff_fallback() {
    let (d, cfg) = staged();
    pwf()
        .args([
            "check",
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
    assert!(
        spawned.contains("git-tools diff"),
        "review task missing bare diff: {spawned}"
    );
    assert!(
        spawned.contains("git-tools diff-subrepos"),
        "review task missing subrepos diff: {spawned}"
    );
    assert!(
        !spawned.contains(".."),
        "fallback review task carried a range: {spawned}"
    );
}

// 9. resolve --show: markdown emitter (PWF-0059).

/// Stage a single-project notes dir with a custom item file and return (dir, cfg).
/// The index entry uses the canonical `- [ ] [[<id>|<title>]]` format.
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
    (dir, cfg)
}

#[test]
fn resolve_show_emits_markdown_without_created_key() {
    let (_d, cfg) = staged_with_item(
        "pwf",
        "PWF",
        "PWF-0001",
        "do the thing",
        "---\nstatus: active\ntitle: do the thing\nproject: pwf\ncreated: 2026-06-20\n---\n\n## Goals\n- do the thing\n",
    );
    let out = pwf()
        .args(["resolve", "--show", "--id", "PWF-0001", "--config-path"])
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
        !stdout.contains("created:"),
        "created key must be stripped: {stdout}"
    );
}

#[test]
fn resolve_show_legacy_item_emits_body_only() {
    // Legacy inline items use the backtick-checkbox format; item_file = None, so
    // resolve --show falls back to the parsed prompt string (no frontmatter to strip).
    let dir = TempDir::new().unwrap();
    let notes = dir.path().join("notes");
    let proj = notes.join("glep-shimeji");
    fs::create_dir_all(&proj).unwrap();
    // Inline legacy format: `- [ ] \`session\` <- prompt` (no per-item .md file).
    fs::write(
        proj.join("glep-shimeji.md"),
        "- [ ] `legacy task` <- do the legacy thing\n",
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
    // Legacy items get ids like "glep-shimeji:1" (ordinal is 1-based).
    let out = pwf()
        .args([
            "resolve",
            "--show",
            "--id",
            "glep-shimeji:1",
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    // Prompt text emitted.
    assert!(
        stdout.contains("do the legacy thing"),
        "prompt not in output: {stdout}"
    );
}

/// Stage a single-project notes dir with a done item parked under `_archive/`
/// (no index entry, mirroring `check`/`cancel`) and return (dir, cfg).
fn staged_with_archived_item(
    project: &str,
    prefix: &str,
    id: &str,
    content: &str,
) -> (TempDir, std::path::PathBuf) {
    let dir = TempDir::new().unwrap();
    let notes = dir.path().join("notes");
    let archive = notes.join(project).join("_archive");
    fs::create_dir_all(&archive).unwrap();
    fs::write(archive.join(format!("{id}.md")), content).unwrap();
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
    (dir, cfg)
}

/// Stage a single-project notes dir with a done item whose note still sits in the
/// project dir while its index link is checked (`- [x]`), as `check` leaves it until
/// the done-queue cap evicts it to `_archive`. Returns (dir, cfg).
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
    // Checked link → the index parser skips it, so find_pending_item misses it.
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
    (dir, cfg)
}

#[test]
fn e2e_reopen_flips_done_item_back_to_active_and_restores_index() {
    // PWF-0054: reopen is the inverse of check — done → active, drop provenance,
    // flip the done-queue link back to an open `- [ ]`.
    let (dir, cfg) = staged_with_done_item(
        "pwf",
        "PWF",
        "PWF-0003",
        "---\nstatus: done\ntitle: just done\nproject: pwf\ncreated: 2026-06-20\ncompleted: 2026-06-20\ncommits: \"a..b\"\n---\n\n## Goals\n- finish it\n",
    );
    pwf()
        // Lowercase id exercises the case-insensitive match + canonical output.
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
    assert_eq!(index, "- [ ] [[PWF-0003]]\n", "index not reopened: {index}");
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
fn resolve_show_finds_done_item_still_in_project_dir() {
    // PWF-0061: a freshly-checked item keeps its note in the project dir but its
    // index link is `- [x]`, so the parser skips it. resolve must still find it.
    let (_d, cfg) = staged_with_done_item(
        "pwf",
        "PWF",
        "PWF-0003",
        "---\nstatus: done\ntitle: just done\nproject: pwf\ncompleted: 2026-06-20\n---\n\n## Goals\n- just done\n",
    );
    let out = pwf()
        .args(["resolve", "--show", "--id", "PWF-0003", "--config-path"])
        .arg(&cfg)
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("status: done"), "missing status: {stdout}");
    assert!(stdout.contains("- just done"), "missing body: {stdout}");
}

#[test]
fn resolve_show_finds_archived_done_item() {
    // PWF-0061: done items are unlinked from the index and parked under `_archive`.
    // resolve --show must still find them so it shows tasks regardless of status.
    let (_d, cfg) = staged_with_archived_item(
        "pwf",
        "PWF",
        "PWF-0002",
        "---\nstatus: done\ntitle: finished thing\nproject: pwf\ncreated: 2026-06-20\n---\n\n## Goals\n- finished thing\n",
    );
    let out = pwf()
        // Lowercase id also exercises the case-insensitive archive match.
        .args(["resolve", "--show", "--id", "pwf-0002", "--config-path"])
        .arg(&cfg)
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("status: done"), "missing status: {stdout}");
    assert!(
        stdout.contains("- finished thing"),
        "missing body: {stdout}"
    );
    assert!(
        !stdout.contains("created:"),
        "created key must be stripped: {stdout}"
    );
}

#[test]
fn resolve_prints_archived_item_path() {
    let (_d, cfg) = staged_with_archived_item(
        "pwf",
        "PWF",
        "PWF-0002",
        "---\nstatus: cancelled\ntitle: dropped\nproject: pwf\n---\n\n## Goals\n- dropped\n",
    );
    let out = pwf()
        .args(["resolve", "--id", "PWF-0002", "--config-path"])
        .arg(&cfg)
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.contains("_archive/PWF-0002.md"),
        "path should point at the archived note: {stdout}"
    );
}

#[test]
fn resolve_errors_when_id_absent_from_index_and_archive() {
    let (_d, cfg) = staged_with_archived_item(
        "pwf",
        "PWF",
        "PWF-0002",
        "---\nstatus: done\ntitle: t\nproject: pwf\n---\n\nbody\n",
    );
    pwf()
        .args(["resolve", "--id", "PWF-9999", "--config-path"])
        .arg(&cfg)
        .assert()
        .failure();
}

// 9b. show: shorthand alias for `resolve --show` (PWF-0065).

#[test]
fn show_streams_note_markdown_like_resolve_show() {
    // `show --id` == `resolve --show --id`: emit the note markdown, minus the
    // execution-irrelevant `created` key.
    let (_d, cfg) = staged_with_item(
        "pwf",
        "PWF",
        "PWF-0001",
        "do the thing",
        "---\nstatus: active\ntitle: do the thing\nproject: pwf\ncreated: 2026-06-20\n---\n\n## Goals\n- do the thing\n",
    );
    let out = pwf()
        // Bare positional id — no `--id` flag.
        .args(["show", "PWF-0001", "--config-path"])
        .arg(&cfg)
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.contains("status: active"),
        "missing status: {stdout}"
    );
    assert!(stdout.contains("## Goals"), "missing body: {stdout}");
    assert!(
        !stdout.contains("created:"),
        "created key must be stripped: {stdout}"
    );
}

#[test]
fn show_finds_archived_done_item_regardless_of_status() {
    // The alias inherits resolve's status-agnostic lookup: a done item evicted to
    // `_archive` (no index link) is still found and streamed.
    let (_d, cfg) = staged_with_archived_item(
        "pwf",
        "PWF",
        "PWF-0002",
        "---\nstatus: done\ntitle: finished thing\nproject: pwf\ncreated: 2026-06-20\n---\n\n## Goals\n- finished thing\n",
    );
    let out = pwf()
        // Lowercase positional id also exercises the case-insensitive archive match.
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
fn show_without_id_errors_with_clean_usage_hiding_pw_engine() {
    // PWF-0065: missing the required positional is a clap error; its usage must read
    // `pwf show …`, never leaking the internal `pw` engine token the preprocess injects.
    let out = pwf().arg("show").assert().failure();
    let stderr = String::from_utf8(out.get_output().stderr.clone()).unwrap();
    assert!(
        stderr.contains("pwf show"),
        "usage should name the verb: {stderr}"
    );
    assert!(
        !stderr.contains("pwf pw"),
        "usage must not leak the internal pw engine: {stderr}"
    );
}

// 10. update --commits: amend provenance, incl. on closed items (PWF-0062).

#[test]
fn update_commits_amends_done_item_in_project_dir() {
    // PWF-0062: a closed item (checked `- [x]` link, note still in project dir) is
    // skipped by find_pending_item; update --commits must still amend its provenance.
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
    // Verify via resolve --show: the commits line is overwritten, status untouched.
    let out = pwf()
        .args(["resolve", "--show", "--id", "PWF-0003", "--config-path"])
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
        .args(["resolve", "--show", "--id", "PWF-0002", "--config-path"])
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
        "---\nstatus: active\ntitle: do the thing\nproject: pwf\ncreated: 2026-06-20\n---\n\n## Goals\n- do the thing\n",
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
        .args(["resolve", "--show", "--id", "PWF-0001", "--config-path"])
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
    // PWF-0065: a closed item needs a multi-section narrative closeout report;
    // --append-report must append it verbatim to the body without rerunning the
    // title/Goals regeneration that --prompt does.
    let (_d, cfg) = staged_with_done_item(
        "pwf",
        "PWF",
        "PWF-0003",
        "---\nstatus: done\ntitle: t\nproject: pwf\ncompleted: 2026-06-20\n---\n\n## Goals\n\n- ship it\n",
    );
    let report =
        "## Outcome\n\nShipped `--append-report`.\n\n## Follow-ups\n\n- write the FSD card";
    pwf()
        .args(["update", "--id", "pwf-0003", "--append-report"])
        .arg(report)
        .arg("--config-path")
        .arg(&cfg)
        .assert()
        .success()
        .stdout(contains("report appended"));
    let note = fs::read_to_string(_d.path().join("notes/pwf/PWF-0003.md")).unwrap();
    // Body and frontmatter untouched; status stays done (no regeneration).
    assert!(note.contains("status: done"), "status changed: {note}");
    assert!(
        note.contains("## Goals\n\n- ship it\n"),
        "body altered: {note}"
    );
    // Report appended verbatim — headings, blank lines, and list survive.
    assert!(
        note.contains(
            "### Report\n\n## Outcome\n\nShipped `--append-report`.\n\n## Follow-ups\n\n- write the FSD card\n"
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

// 10a. update --append/-a: splice lane-syntax bullets into the body (PWF-0090).

#[test]
fn update_append_splices_bullets_into_an_existing_section() {
    let (_d, cfg) = staged_with_item(
        "pwf",
        "PWF",
        "PWF-0001",
        "do the thing",
        "---\nstatus: active\ntitle: do the thing\nproject: pwf\ncreated: 2026-06-20\n---\n\n## Goals\n- do the thing\n",
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
        "---\nstatus: active\ntitle: do the thing\nproject: pwf\ncreated: 2026-06-20\n---\n\n## Goals\n- do the thing\n",
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
fn update_append_rejects_whitespace_only() {
    let (_d, cfg) = staged_with_item(
        "pwf",
        "PWF",
        "PWF-0001",
        "do the thing",
        "---\nstatus: active\ntitle: do the thing\nproject: pwf\ncreated: 2026-06-20\n---\n\n## Goals\n- do the thing\n",
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
        "---\nstatus: active\ntitle: do the thing\nproject: pwf\ncreated: 2026-06-20\n---\n\n## Goals\n- do the thing\n",
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

// 11. session verb: zellij-independent error surfaces (PWF-0038).

/// Stage a launchable PWF-0001 item with a real repo dir plus a recording `zellij`
/// stub on a child PATH. Returns the cfg path, the child `PATH`, and the argv log
/// the stub appends to — the shared rig for the session argv-capture e2e tests.
#[cfg(unix)]
fn stage_session_with_zellij_stub(
    dir: &TempDir,
) -> (std::path::PathBuf, String, std::path::PathBuf) {
    use std::os::unix::fs::PermissionsExt;

    // A REAL repo dir: the session preflight RepoMissing-rejects a nonexistent repo.
    let notes = dir.path().join("notes");
    let proj = notes.join("pwf");
    let repo = dir.path().join("repo");
    fs::create_dir_all(&proj).unwrap();
    fs::create_dir_all(&repo).unwrap();
    fs::write(
        proj.join("PWF-0001.md"),
        "---\nstatus: active\ntitle: do the thing\nproject: pwf\ncreated: 2026-06-20\n---\n\n## Goals\n- do the thing\n",
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

    // A recording `zellij` on the child's PATH: copy the fixture to <bin>/zellij, +x.
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

    // The title travels ONLY in zellij's argv (claude's --name value), never stdout.
    let argv = fs::read_to_string(&log).unwrap();
    assert!(
        argv.contains("PWF-0001 - do the thing"),
        "thread title not in captured zellij argv: {argv}"
    );
}

#[test]
#[cfg(unix)]
fn session_worktree_flag_injects_instruction_into_argv() {
    // PWF-0076: `-w`/`--worktree` augments the launch prompt with a git-worktree
    // setup step naming the item id. It rides ONLY in the dispatched agent's argv.
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
    // Without `-w`, the launch prompt carries no worktree step (default-off).
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
    // Re-stage the item note with an effort tag (stage_session_with_zellij_stub's
    // PWF-0001.md has no effort: line; append one so this test doesn't need its
    // own full staging duplicate).
    let notes = dir.path().join("notes");
    fs::write(
        notes.join("pwf").join("PWF-0001.md"),
        "---\nstatus: active\ntitle: do the thing\nproject: pwf\ncreated: 2026-06-20\neffort: 4\n---\n\n## Goals\n- do the thing\n",
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
fn session_with_effort_and_broken_tiers_config_fails_before_dispatch() {
    let dir = TempDir::new().unwrap();
    let (cfg, path, log) = stage_session_with_zellij_stub(&dir);
    let notes = dir.path().join("notes");
    fs::write(
        notes.join("pwf").join("PWF-0001.md"),
        "---\nstatus: active\ntitle: do the thing\nproject: pwf\ncreated: 2026-06-20\neffort: 4\n---\n\n## Goals\n- do the thing\n",
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

    // Nothing dispatched: the stub never logged a zellij call.
    assert!(!log.exists() || fs::read_to_string(&log).unwrap().is_empty());
}

#[test]
#[cfg(unix)]
fn session_codex_agent_emits_codex_argv() {
    // PWF-0079: Codex has no `--name` flag. `pwf` launches it through a small
    // title-aware shim that renames the Codex thread via Codex's app-server API,
    // then runs `codex -- <prompt>`.
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
    // The real Codex command still runs with the prompt as a `--`-guarded
    // positional. zellij's own `--name <tab>` is always present; assert on the
    // codex segment rather than the whole line.
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
    // PWF-0088: `-a`/`--append` on session reuses `update`'s lane-syntax splice to
    // extend the body in place before dispatching. PWF-0093: the launch prompt is
    // a thin pointer, not the note body, so the extension lands in the note (which
    // the dispatched agent resolves itself) rather than riding in the argv.
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
    // PWF-0093: the pw-workflow skill's first step already resolves the item and
    // reads its body in full, so the launch prompt just names the id/project and
    // points the agent at the task — it must not inline the note's Goals/Context.
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
    // PWF-0088: session's `-a` is `--append`, not `--agent` (the agent shorthand
    // was removed to free it). `-a <text>` must splice into the body and still
    // dispatch the default claude launcher.
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
    // clap ValueEnum rejects an unknown --agent value before any dispatch.
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
    // PWF-0068: `pwf verify --agent codex` probes codex and renders the codex
    // command. `command: codex` is host-independent (no item → binary-only command);
    // `codex:` appears whether or not codex resolves on this host's PATH.
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
        .success() // `verify` itself still exits 0 — it reports "fail" in its markdown, doesn't hard-error.
        .stdout(contains("\u{2014} fail"))
        .stdout(contains("launchable: no"));
}

#[test]
fn session_missing_id_errors() {
    // `pwf session` with no id → MissingId, before any zellij probe.
    pwf()
        .arg("session")
        .assert()
        .failure()
        .stderr(contains("--id is required for session"));
}

#[test]
fn session_unknown_id_errors_not_found() {
    // Unknown id resolves to not-found before the zellij availability check.
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
    // PWF-0074: `--yes` is a valid session flag; it doesn't alter id resolution,
    // so an unknown id still fails not-found (proving the flag parsed, not errored).
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
    // PWF-0073: `-i` is a valid session flag; it doesn't alter id resolution, so an
    // unknown id still fails not-found (proving the flag parsed and validation runs
    // before any exec — no zellij/claude needed in CI).
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
    // PWF-0073: the long `--inline` form parses identically.
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
    // PWF-0076: `-w`/`--worktree` is a valid session flag; it doesn't alter id
    // resolution, so an unknown id still fails not-found (proving the flag parsed).
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
    // `launch` is no longer a clap subcommand; the preprocessor treats it as a
    // route word (project name), which fails as an unknown managed project.
    pwf()
        .args(["launch", "--id", "GLP-0001"])
        .assert()
        .failure()
        .stderr(contains("Unknown managed project identifier"));
}

#[test]
fn retired_launch_claude_verb_treated_as_unknown_project() {
    // `launch-claude` is no longer a clap subcommand; same routing as above.
    pwf()
        .args(["launch-claude", "--id", "GLP-0001"])
        .assert()
        .failure()
        .stderr(contains("Unknown managed project identifier"));
}

// ── PWF-0081: note engine — add/ls/remove e2e ────────────────────────────────

#[test]
fn note_add_list_update_remove_preserves_tasks_and_header() {
    // Stage a `pwf` project with a seeded task; verifies note mutations leave the
    // task line and `### Notes` header intact across the full add→ls→update→remove cycle.
    let (dir, cfg) = staged_with_item(
        "pwf",
        "PWF",
        "PWF-0001",
        "real task",
        "---\nstatus: active\ntitle: real task\nproject: pwf\ncreated: 2026-01-01\n---\n\nbody\n",
    );

    pwf()
        .args(["note", "pwf", "--config-path"])
        .arg(&cfg)
        .args(["add", "remember the milk"])
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

    pwf()
        .args(["note", "pwf", "--config-path"])
        .arg(&cfg)
        .args(["update", "1", "remember oat milk"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "Updated PWF-NOTE-0001 :: remember oat milk",
        ));

    pwf()
        .args(["note", "pwf", "--config-path"])
        .arg(&cfg)
        .arg("ls")
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
        .args(["note", "pwf", "--config-path"])
        .arg(&cfg)
        .args(["remove", "1"])
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
