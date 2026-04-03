//! Exec-level checks for help dispatch. `CARGO_BIN_EXE_pwf` is injected by Cargo
//! for integration tests of the `pwf` binary.
use std::fs;
use std::process::Command;

fn run(args: &[&str]) -> (String, bool) {
    let out = Command::new(env!("CARGO_BIN_EXE_pwf"))
        .args(args)
        .output()
        .expect("run pwf");
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        out.status.success(),
    )
}

fn nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}

fn stage_dir() -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("pwcli_{}", nanos()));
    fs::create_dir_all(&d).unwrap();
    d
}

fn json_path(p: &std::path::Path) -> String {
    p.to_string_lossy().replace('\\', "\\\\")
}

fn stage_remove_item() -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
    let stage = stage_dir();
    let notes = stage.join("notes");
    let repo = stage.join("repo");
    let project = notes.join("pwf");
    fs::create_dir_all(&project).unwrap();
    fs::create_dir_all(&repo).unwrap();
    fs::write(
        project.join("PWF-0001.md"),
        "---\nstatus: active\ntitle: stale task\nproject: pwf\ncreated: 2026-01-01\n---\n\nremove me\n",
    )
    .unwrap();
    fs::write(
        project.join("pwf.md"),
        "- [ ] [[PWF-0001|stale task]]\n- [ ] [[PWF-0002|keep task]]\n",
    )
    .unwrap();
    let cfg = stage.join("config.json");
    fs::write(
        &cfg,
        format!(
            r#"{{ "notesDir": "{}", "projects": {{ "pwf": "{}" }}, "prefixes": {{ "pwf": "PWF" }} }}"#,
            json_path(&notes),
            json_path(&repo)
        ),
    )
    .unwrap();
    (notes, project, cfg)
}

#[test]
fn per_engine_help_is_scoped_and_succeeds() {
    let (pw, ok) = run(&["pw", "--help"]);
    assert!(ok, "pwf pw --help should exit 0");
    assert!(
        pw.contains("launch-claude"),
        "pw --help should list pw actions"
    );
    assert!(pw.contains("remove"), "pw --help should list remove");
    assert!(!pw.contains("handoff ledger"), "pw help is scoped to pw");

    let (ho, ok) = run(&["handoff", "--help"]);
    assert!(ok);
    assert!(
        ho.contains("ledger"),
        "handoff help should mention its ledger"
    );
    assert!(
        !ho.contains("launch-claude"),
        "handoff help is scoped to handoff"
    );
}

#[test]
fn canonical_remove_deletes_item_from_cli() {
    let (notes, project, cfg) = stage_remove_item();

    let out = Command::new(env!("CARGO_BIN_EXE_pwf"))
        .args([
            "pw",
            "remove",
            "--id",
            "pwf-0001",
            "--json",
            "--notes-dir",
            &notes.to_string_lossy(),
        ])
        .env("PWF_CONFIG", &cfg)
        .output()
        .expect("run pwf");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "stdout: {stdout}\nstderr: {stderr}");
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["status"], "removed");
    assert!(!project.join("PWF-0001.md").exists());
    let index = fs::read_to_string(project.join("pwf.md")).unwrap();
    assert!(
        !index.contains("PWF-0001"),
        "removed link retained: {index}"
    );
    assert!(
        index.contains("PWF-0002"),
        "unrelated link removed: {index}"
    );
}

#[test]
fn bare_pw_engine_lists_pending_work() {
    let stage = stage_dir();
    let notes = stage.join("notes");
    let proj = notes.join("glep-shimeji");
    fs::create_dir_all(&proj).unwrap();
    fs::write(
        proj.join("GLP-0001.md"),
        "---\nstatus: active\ntitle: tray gui\nproject: glep-shimeji\ncreated: 2026-01-01\n---\n\nadd startup toggle\n",
    )
    .unwrap();
    fs::write(
        proj.join("glep-shimeji.md"),
        "- [ ] [[GLP-0001|tray gui]]\n",
    )
    .unwrap();
    let cfg = stage.join("config.json");
    fs::write(
        &cfg,
        format!(
            r#"{{ "notesDir": "{}", "projects": {{ "glep-shimeji": "/repo" }}, "prefixes": {{ "glep-shimeji": "GLP" }} }}"#,
            json_path(&notes)
        ),
    )
    .unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_pwf"))
        .arg("pw")
        .env("PWF_CONFIG", &cfg)
        .output()
        .expect("run pwf");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "stderr: {stderr}");
    assert!(stdout.contains("[GLP-0001] glep-shimeji :: tray gui"));
}

#[test]
fn bare_pw_engine_treats_next_arg_as_project() {
    let stage = stage_dir();
    let notes = stage.join("notes");
    let glep = notes.join("glep-shimeji");
    let config = notes.join("config-handler");
    fs::create_dir_all(&glep).unwrap();
    fs::create_dir_all(&config).unwrap();
    fs::write(
        glep.join("GLP-0001.md"),
        "---\nstatus: active\ntitle: tray gui\nproject: glep-shimeji\ncreated: 2026-01-01\n---\n\nadd startup toggle\n",
    )
    .unwrap();
    fs::write(
        glep.join("glep-shimeji.md"),
        "- [ ] [[GLP-0001|tray gui]]\n",
    )
    .unwrap();
    fs::write(
        config.join("CFG-0001.md"),
        "---\nstatus: active\ntitle: config task\nproject: config-handler\ncreated: 2026-01-01\n---\n\nfix config\n",
    )
    .unwrap();
    fs::write(
        config.join("config-handler.md"),
        "- [ ] [[CFG-0001|config task]]\n",
    )
    .unwrap();
    let cfg = stage.join("config.json");
    fs::write(
        &cfg,
        format!(
            r#"{{ "notesDir": "{}", "projects": {{ "config-handler": "/config", "glep-shimeji": "/glep" }}, "prefixes": {{ "config-handler": "CFG", "glep-shimeji": "GLP" }} }}"#,
            json_path(&notes)
        ),
    )
    .unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_pwf"))
        .args(["pw", "config-handler"])
        .env("PWF_CONFIG", &cfg)
        .output()
        .expect("run pwf");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "stderr: {stderr}");
    assert!(stdout.contains("[CFG-0001] config-handler :: config task"));
    assert!(!stdout.contains("[GLP-0001]"), "got: {stdout}");
}

#[test]
fn list_is_an_alias_for_help() {
    let (a, _) = run(&["--list"]);
    let (b, _) = run(&["--help"]);
    assert_eq!(a, b, "--list output should equal --help output");
}

#[test]
fn terse_help_is_lean_and_succeeds() {
    let (terse, ok) = run(&["pw", "--help", "--terse"]);
    assert!(ok, "pwf pw --help --terse should exit 0");
    assert!(
        terse.contains("add <project> <prompt>"),
        "terse should list verbs+args"
    );
    assert!(!terse.contains("[just "), "terse should drop recipe hints");
    assert!(
        !terse.contains("List open items."),
        "terse should drop descriptions"
    );

    let (full, _) = run(&["pw", "--help"]);
    assert!(
        terse.len() < full.len(),
        "terse ({}) should be leaner than the rich clap help ({})",
        terse.len(),
        full.len()
    );
}
