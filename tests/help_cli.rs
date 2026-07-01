//! Exec-level checks for help dispatch. `CARGO_BIN_EXE_pwf` is injected by Cargo
//! for integration tests of the `pwf` binary.
use std::{fs, process::Command};

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
fn per_action_help_is_scoped_and_succeeds() {
    let (pw_help, ok) = run(&["add", "--help"]);
    assert!(ok, "pwf add --help should exit 0");
    assert!(
        pw_help.contains("Add a pwf task"),
        "add --help should include pending-work description"
    );
    assert!(
        pw_help.contains("project"),
        "add --help should include the project arg name"
    );
    assert!(
        pw_help.contains("/c <context>") && pw_help.contains("/n <constraint>"),
        "add --help should mention rich prompt lane markers: {pw_help}"
    );
    assert!(
        !pw_help.contains("handoff ledger"),
        "pw help should stay pending-work scoped"
    );

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
            "remove",
            "--id",
            "pwf-0001",
            "--notes-dir",
            &notes.to_string_lossy(),
        ])
        .env("PWF_CONFIG", &cfg)
        .output()
        .expect("run pwf");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "stdout: {stdout}\nstderr: {stderr}");
    assert!(
        stdout.contains("REMOVED PWF TASK [PWF-0001]") && stdout.contains("pwf :: stale task"),
        "expected remove confirmation for PWF-0001: {stdout}"
    );
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
fn default_engine_lists_pending_work() {
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
        .args(["list"])
        .env("PWF_CONFIG", &cfg)
        .output()
        .expect("run pwf");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "stderr: {stderr}");
    assert!(stdout.contains("GLP-0001 :: tray gui"));
}

#[test]
fn default_engine_treats_next_arg_as_project() {
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
        .args(["config-handler"])
        .env("PWF_CONFIG", &cfg)
        .output()
        .expect("run pwf");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "stderr: {stderr}");
    assert!(stdout.contains("CFG-0001 :: config task"));
    assert!(!stdout.contains("GLP-0001"), "got: {stdout}");
}

#[test]
fn list_is_an_alias_for_help() {
    let (a, _) = run(&["--list"]);
    let (b, _) = run(&["--help"]);
    assert_eq!(a, b, "--list output should equal --help output");
}

#[test]
fn top_level_help_groups_default_commands_and_engines() {
    let (help, ok) = run(&["--help"]);
    assert!(ok, "pwf --help should exit 0");
    assert!(
        help.contains("Commands:"),
        "top help should be clap-rendered command help: {help}"
    );
    assert!(
        help.contains("pw"),
        "top help should include the default pending-work engine: {help}"
    );
    assert!(
        help.contains("handoff"),
        "top help should mention handoff engine: {help}"
    );
    assert!(
        help.contains("migrate"),
        "top help should mention migrate engine: {help}"
    );
    assert!(
        help.contains("note"),
        "top help should mention note engine: {help}"
    );
    assert!(
        !help.contains("LEGACY"),
        "top help should not advertise the removed legacy surface: {help}"
    );
}

#[test]
fn terse_help_is_lean_and_succeeds() {
    let (terse, ok) = run(&["--help", "--terse"]);
    assert!(ok, "pwf --help --terse should exit 0");
    assert!(
        terse.contains("add <project> <prompt>"),
        "terse should list verbs+args"
    );
    assert!(
        terse.contains("/ <goal>") && terse.contains("/d <done>"),
        "terse should advertise prompt lanes: {terse}"
    );
    assert!(
        terse.contains("note <project>"),
        "terse should list the note engine: {terse}"
    );
    assert!(!terse.contains("[just "), "terse should drop recipe hints");
    assert!(
        !terse.contains("List open items."),
        "terse should drop descriptions"
    );

    let (full, _) = run(&["--help"]);
    assert!(
        full.contains("Commands:") && full.contains("note"),
        "rich help should be clap's command index: {full}"
    );
}

#[test]
fn verb_terse_help_is_scoped_to_that_verb() {
    // PWF-0063: `pwf <verb> --help --terse` must show only that verb, not every engine.
    let (terse, ok) = run(&["update", "--help", "--terse"]);
    assert!(ok, "pwf update --help --terse should exit 0");
    assert!(
        terse.trim_start().starts_with("update --id"),
        "should lead with the update verb line: {terse}"
    );
    assert!(
        !terse.contains("handoff <verb>"),
        "verb-scoped terse must not dump the handoff engine: {terse}"
    );
    assert!(
        !terse.contains("resolve --id"),
        "verb-scoped terse must not dump sibling verbs: {terse}"
    );

    // The engine forms and the top-level map still emit their full blocks.
    let (engine, ok) = run(&["handoff", "--help", "--terse"]);
    assert!(ok);
    assert!(
        engine.contains("handoff <verb>"),
        "engine terse stays full: {engine}"
    );
    let (top, _) = run(&["--help", "--terse"]);
    assert!(
        top.contains("handoff <verb>") && top.contains("resolve --id"),
        "top-level terse stays full: {top}"
    );
}

#[test]
fn session_help_documents_yes_flag() {
    // PWF-0074: `--yes`/`-y` skips the [Y/n] dispatch confirmation.
    let (help, ok) = run(&["session", "--help"]);
    assert!(ok, "pwf session --help should exit 0");
    assert!(
        help.contains("--yes"),
        "session help should list --yes: {help}"
    );
    assert!(
        help.contains("confirmation"),
        "session help should describe the confirmation: {help}"
    );

    let (terse, ok) = run(&["session", "--help", "--terse"]);
    assert!(ok, "pwf session --help --terse should exit 0");
    assert!(
        terse.contains("-y"),
        "session terse should mention the -y shortcut: {terse}"
    );
}

#[test]
fn session_help_documents_worktree_flag() {
    // PWF-0076: `-w`/`--worktree` augments the launch prompt with a worktree step.
    let (help, ok) = run(&["session", "--help"]);
    assert!(ok, "pwf session --help should exit 0");
    assert!(
        help.contains("--worktree"),
        "session help should list --worktree: {help}"
    );
    assert!(
        help.contains("worktree"),
        "session help should describe the worktree behavior: {help}"
    );

    let (terse, ok) = run(&["session", "--help", "--terse"]);
    assert!(ok, "pwf session --help --terse should exit 0");
    assert!(
        terse.contains("-w"),
        "session terse should mention the -w shortcut: {terse}"
    );
}

#[test]
fn session_help_documents_auto_flag() {
    // PWF-0077: `--auto` appends an autonomy directive for unattended dispatch.
    let (help, ok) = run(&["session", "--help"]);
    assert!(ok, "pwf session --help should exit 0");
    assert!(
        help.contains("--auto"),
        "session help should list --auto: {help}"
    );
    assert!(
        help.to_lowercase().contains("autonom"),
        "session help should describe the autonomy behavior: {help}"
    );

    let (terse, ok) = run(&["session", "--help", "--terse"]);
    assert!(ok, "pwf session --help --terse should exit 0");
    assert!(
        terse.contains("--auto"),
        "session terse should mention --auto: {terse}"
    );
}

#[test]
fn session_help_documents_agent_flag_long_only() {
    // PWF-0088: `--agent <claude|codex>` selects the agent (claude default); its
    // `-a` shorthand was dropped so `-a` can mean `--append` on session instead.
    let (help, ok) = run(&["session", "--help"]);
    assert!(ok, "pwf session --help should exit 0");
    assert!(
        help.contains("--agent"),
        "session help should list --agent: {help}"
    );
    assert!(
        !help.contains("-a, --agent"),
        "session's --agent must be long-only, -a now means --append: {help}"
    );
}

#[test]
fn session_help_documents_append_flag() {
    // PWF-0088: `-a`/`--append <lanes>` reuses `update`'s lane-syntax splice to
    // extend the body before dispatch.
    let (help, ok) = run(&["session", "--help"]);
    assert!(ok, "pwf session --help should exit 0");
    assert!(
        help.contains("-a, --append <APPEND>"),
        "session help should list -a/--append: {help}"
    );

    let (terse, ok) = run(&["session", "--help", "--terse"]);
    assert!(ok, "pwf session --help --terse should exit 0");
    assert!(
        terse.contains("-a/--append"),
        "session terse should mention -a/--append: {terse}"
    );
}

#[test]
fn verify_help_documents_agent_flag() {
    // PWF-0068: `pwf verify` probes the selected agent (claude default).
    let (help, ok) = run(&["verify", "--help"]);
    assert!(ok, "pwf verify --help should exit 0");
    assert!(
        help.contains("--agent"),
        "verify help should list --agent: {help}"
    );

    let (terse, ok) = run(&["verify", "--help", "--terse"]);
    assert!(ok, "pwf verify --help --terse should exit 0");
    assert!(
        terse.contains("-a"),
        "verify terse should mention the -a shortcut: {terse}"
    );
}

#[test]
fn list_help_mentions_all_scope() {
    let (list_help, ok) = run(&["list", "--help"]);
    assert!(ok, "pwf list --help should exit 0");
    assert!(
        list_help.contains("--all"),
        "list --help should document --all: {list_help}"
    );

    let (terse, ok) = run(&["--help", "--terse"]);
    assert!(ok, "pwf --help --terse should exit 0");
    assert!(
        terse.contains("[--all]"),
        "terse help should document --all: {terse}"
    );
}

#[test]
fn json_flag_is_rejected() {
    let out = Command::new(env!("CARGO_BIN_EXE_pwf"))
        .args(["list", "--json"])
        .output()
        .expect("run pwf");
    assert!(!out.status.success(), "pwf list --json should fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("unexpected argument") || stderr.contains("--json"),
        "stderr should mention --json: {stderr}"
    );
}
