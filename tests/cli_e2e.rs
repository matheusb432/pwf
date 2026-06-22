//! Behavior-level checks of the built binary (clap surface), per the
//! rust-cli-tooling testing matrix: exit codes, that the retired legacy flag
//! surface now errors, and canonical-only flags.

use assert_cmd::Command;
use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;
use std::fs;
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
            "a & b",
            "--date",
            "2026-01-01",
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .success();
    let item = read_item(&d, "GLP-0001");
    // Body is the Goals template, one bullet per `&` segment.
    assert!(
        item.contains("Goals:\n- a\n- b"),
        "body not Goals-wrapped: {item}"
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
    assert!(item.contains("Goals:\n- fresh prompt"), "body: {item}");
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

// 2. `&` cut + Goals split via the binary (mirrors pending_work.rs:134-189).

#[test]
fn e2e_add_cuts_title_at_ampersand_and_splits_goals() {
    let (d, cfg) = staged();
    pwf()
        .args([
            "add",
            "glep-shimeji",
            "lead clause & goal two & goal three",
            "--config-path",
        ])
        .arg(&cfg)
        .assert()
        .success();
    let item = read_item(&d, "GLP-0002");
    // Title is the lead clause, lowercased, cut at the first `&`.
    assert_eq!(title_of(&item), "lead clause", "title not cut: {item}");
    // One bullet per `&` segment.
    assert!(
        item.contains("Goals:\n- lead clause\n- goal two\n- goal three"),
        "goals not split per segment: {item}"
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
        "---\nstatus: active\ntitle: do the thing\nproject: pwf\ncreated: 2026-06-20\n---\n\nGoals:\n- do the thing\n",
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
    assert!(stdout.contains("Goals:"), "missing body: {stdout}");
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
fn resolve_show_finds_done_item_still_in_project_dir() {
    // PWF-0061: a freshly-checked item keeps its note in the project dir but its
    // index link is `- [x]`, so the parser skips it. resolve must still find it.
    let (_d, cfg) = staged_with_done_item(
        "pwf",
        "PWF",
        "PWF-0003",
        "---\nstatus: done\ntitle: just done\nproject: pwf\ncompleted: 2026-06-20\n---\n\nGoals:\n- just done\n",
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
        "---\nstatus: done\ntitle: finished thing\nproject: pwf\ncreated: 2026-06-20\n---\n\nGoals:\n- finished thing\n",
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
        "---\nstatus: cancelled\ntitle: dropped\nproject: pwf\n---\n\nGoals:\n- dropped\n",
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
        "---\nstatus: active\ntitle: do the thing\nproject: pwf\ncreated: 2026-06-20\n---\n\nGoals:\n- do the thing\n",
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
    assert!(stdout.contains("Goals:"), "missing body: {stdout}");
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
        "---\nstatus: done\ntitle: finished thing\nproject: pwf\ncreated: 2026-06-20\n---\n\nGoals:\n- finished thing\n",
    );
    let out = pwf()
        // Lowercase id also exercises the case-insensitive archive match.
        .args(["show", "--id", "pwf-0002", "--config-path"])
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
        .args(["show", "--id", "PWF-9999", "--config-path"])
        .arg(&cfg)
        .assert()
        .failure();
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
        "---\nstatus: active\ntitle: do the thing\nproject: pwf\ncreated: 2026-06-20\n---\n\nGoals:\n- do the thing\n",
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
        .stderr(contains("only --commits can amend closed item"));
}
