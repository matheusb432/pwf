use std::{fmt::Write, fs};

use pwf::engines::pending_work;

#[path = "support/pending_work.rs"]
mod pending_work_test;

use pending_work_test::run_plain;

#[test]
fn add_allocates_first_id_and_writes_files() {
    let stage = stage_dir();
    let notes = stage.join("notes");
    fs::create_dir_all(&notes).unwrap();
    let cfg = stage.join("config.json");
    fs::write(
        &cfg,
        format!(
            r#"{{ "notesDir": "{}", "projects": {{ "glep-shimeji": "/repo" }}, "prefixes": {{ "glep-shimeji": "GLP" }} }}"#,
            json_path(&notes)
        ),
    )
    .unwrap();
    let args = parse_args(&[
        "add",
        "glep-shimeji",
        "add startup toggle",
        "--title",
        "tray gui",
        "--config-path",
        &cfg.to_string_lossy(),
        "--notes-dir",
        &notes.to_string_lossy(),
        "--date",
        "2026-01-01",
    ]);
    let out = run_plain(&args).unwrap();
    assert!(out.starts_with("Added pwf task: **GLP-0001"), "got: {out}");
    let item = fs::read_to_string(stage.join("notes/glep-shimeji/GLP-0001.md")).unwrap();
    assert!(item.contains("status: active"));
    let index = fs::read_to_string(stage.join("notes/glep-shimeji/glep-shimeji.md")).unwrap();
    assert!(
        index.contains("- [ ] [[GLP-0001]]"),
        "bare link written: {index}"
    );
    assert!(!index.contains("[[GLP-0001|"), "no alias written: {index}");
}

#[test]
fn add_allocates_after_closed_items_in_project_directory() {
    const PROJECT: &str = "config-handler";

    let stage = stage_dir();
    let notes = stage.join("notes");
    let proj = notes.join(PROJECT);
    fs::create_dir_all(&proj).unwrap();
    fs::write(
        proj.join("CFG-0088.md"),
        format!(
            "---\nstatus: active\ntitle: active\nproject: {PROJECT}\ncreated: 2026-01-01\n---\n\nbody\n"
        ),
    )
    .unwrap();
    fs::write(
        proj.join("CFG-0089.md"),
        format!(
            "---\nstatus: done\ntitle: closed\nproject: {PROJECT}\ncreated: 2026-01-01\n---\n\nbody\n"
        ),
    )
    .unwrap();
    let cfg = stage.join("config.json");
    fs::write(
        &cfg,
        format!(
            r#"{{ "notesDir": "{notes}", "projects": {{ "{project}": "/repo" }}, "prefixes": {{ "{project}": "CFG" }} }}"#,
            notes = json_path(&notes),
            project = PROJECT
        ),
    )
    .unwrap();
    let args = parse_args(&[
        "add",
        PROJECT,
        "avoid closed duplicate",
        "--config-path",
        &cfg.to_string_lossy(),
        "--notes-dir",
        &notes.to_string_lossy(),
        "--date",
        "2026-01-01",
    ]);

    let out = run_plain(&args).unwrap();
    assert!(out.starts_with("Added pwf task: **CFG-0090"), "got: {out}");
    assert!(proj.join("CFG-0090.md").exists());
    assert!(proj.join("CFG-0089.md").exists());
}

#[test]
fn add_caps_inferred_title_for_long_prompt_without_ampersand() {
    // Keep this at the add boundary because it requires title inference and persisted output.
    let stage = stage_dir();
    let notes = stage.join("notes");
    fs::create_dir_all(&notes).unwrap();
    let cfg = stage.join("config.json");
    fs::write(
        &cfg,
        format!(
            r#"{{ "notesDir": "{}", "projects": {{ "config-handler": "/repo" }}, "prefixes": {{ "config-handler": "CFG" }} }}"#,
            json_path(&notes)
        ),
    )
    .unwrap();
    let long_prompt = "Continue the PowerShell to Rust port into the cfgtool CLI using the shipped gaming domain as the template, porting domain-by-domain smallest first then rewiring the just recipes";
    let args = parse_args(&[
        "add",
        "config-handler",
        long_prompt,
        "--config-path",
        &cfg.to_string_lossy(),
        "--notes-dir",
        &notes.to_string_lossy(),
        "--date",
        "2026-01-01",
    ]);
    run_plain(&args).unwrap();

    let item = fs::read_to_string(stage.join("notes/config-handler/CFG-0001.md")).unwrap();
    let title = item
        .lines()
        .find_map(|l| l.strip_prefix("title: "))
        .expect("title frontmatter present");
    assert!(
        title.chars().count() <= 81,
        "title must stay bounded, got {} chars: {title}",
        title.chars().count()
    );
    assert!(
        title.ends_with('…'),
        "truncated title carries an ellipsis: {title}"
    );
    assert!(
        !title.contains("smallest first"),
        "tail of prompt dropped from title: {title}"
    );
    assert!(
        item.contains("smallest first then rewiring the just recipes"),
        "body keeps full prompt"
    );
}

#[test]
fn add_human_flag_routes_item_under_human_section() {
    let stage = stage_dir();
    let notes = stage.join("notes");
    let proj = notes.join("glep-shimeji");
    fs::create_dir_all(&proj).unwrap();
    fs::write(
        proj.join("GLP-0001.md"),
        "---\nstatus: active\ntitle: existing\nproject: glep-shimeji\ncreated: 2026-01-01\n---\n\nbody\n",
    )
    .unwrap();
    fs::write(
        proj.join("glep-shimeji.md"),
        "- [ ] [[GLP-0001|existing]]\n\n## Human\n\n- [x] keep prior human entry\n\n### Notes\nkeep personal notes here\n",
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
    let args = parse_args(&[
        "add",
        "glep-shimeji",
        "review the thing",
        "--title",
        "human task",
        "--human",
        "--config-path",
        &cfg.to_string_lossy(),
        "--notes-dir",
        &notes.to_string_lossy(),
        "--date",
        "2026-01-01",
    ]);
    run_plain(&args).unwrap();
    let index = fs::read_to_string(proj.join("glep-shimeji.md")).unwrap();
    let human_idx = index
        .find("## Human")
        .unwrap_or_else(|| panic!("no ## Human: {index}"));
    let item_idx = index
        .find("[[GLP-0002]]")
        .unwrap_or_else(|| panic!("no item: {index}"));
    assert!(item_idx > human_idx, "new item not under ## Human: {index}");
    assert!(
        index.contains("## Human\n- [ ] [[GLP-0002]]\n\n- [x] keep prior human entry"),
        "new item not immediately after ## Human: {index}"
    );
    assert!(
        item_idx < index.find("### Notes").unwrap(),
        "new item landed under personal notes: {index}"
    );
    assert!(
        index.find("[[GLP-0001|existing]]").unwrap() < human_idx,
        "normal item moved: {index}"
    );
}

#[test]
fn add_with_prereq_writes_validated_frontmatter() {
    let stage = stage_dir();
    let notes = stage.join("notes");
    let proj = notes.join("glep-shimeji");
    fs::create_dir_all(&proj).unwrap();
    fs::write(
        proj.join("GLP-0001.md"),
        "---\nstatus: active\ntitle: prerequisite\nproject: glep-shimeji\ncreated: 2026-01-01\n---\n\nbody\n",
    )
    .unwrap();
    fs::write(
        proj.join("glep-shimeji.md"),
        "- [ ] [[GLP-0001|prerequisite]]\n",
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
    let args = parse_args(&[
        "add",
        "glep-shimeji",
        "do dependent work",
        "--title",
        "dependent",
        "--prereq",
        "GLP-0001",
        "--config-path",
        &cfg.to_string_lossy(),
        "--notes-dir",
        &notes.to_string_lossy(),
        "--date",
        "2026-01-01",
    ]);
    let out = run_plain(&args).unwrap();
    assert!(out.starts_with("Added pwf task: **GLP-0002"), "got: {out}");
    let item = fs::read_to_string(proj.join("GLP-0002.md")).unwrap();
    assert!(
        item.contains("prereq: \"[[GLP-0001]]\"\n---"),
        "got: {item}"
    );
}

#[test]
fn add_rejects_unknown_prereq_without_writing_item() {
    let stage = stage_dir();
    let notes = stage.join("notes");
    let proj = notes.join("glep-shimeji");
    fs::create_dir_all(&proj).unwrap();
    fs::write(proj.join("glep-shimeji.md"), "").unwrap();
    let cfg = stage.join("config.json");
    fs::write(
        &cfg,
        format!(
            r#"{{ "notesDir": "{}", "projects": {{ "glep-shimeji": "/repo" }}, "prefixes": {{ "glep-shimeji": "GLP" }} }}"#,
            json_path(&notes)
        ),
    )
    .unwrap();
    let args = parse_args(&[
        "add",
        "glep-shimeji",
        "do dependent work",
        "--title",
        "dependent",
        "--prereq",
        "GLP-9999",
        "--config-path",
        &cfg.to_string_lossy(),
        "--notes-dir",
        &notes.to_string_lossy(),
        "--date",
        "2026-01-01",
    ]);
    let err = run_plain(&args).unwrap_err();
    assert!(err.contains("GLP-9999"), "got: {err}");
    assert!(
        !proj.join("GLP-0001.md").exists(),
        "failed add wrote a new item"
    );
}

#[test]
fn add_unmanaged_project_returns_error() {
    let stage = stage_dir();
    let notes = stage.join("notes");
    fs::create_dir_all(&notes).unwrap();
    let cfg = stage.join("config.json");
    fs::write(
        &cfg,
        format!(
            r#"{{ "notesDir": "{}", "projects": {{ "rogue-project": "/repo" }}, "prefixes": {{}} }}"#,
            json_path(&notes)
        ),
    )
    .unwrap();
    let args = parse_args(&[
        "add",
        "rogue-project",
        "do work",
        "--title",
        "unmapped item",
        "--config-path",
        &cfg.to_string_lossy(),
        "--notes-dir",
        &notes.to_string_lossy(),
        "--date",
        "2026-01-01",
    ]);
    let err = run_plain(&args).unwrap_err();
    assert!(err.contains("no work-item prefix"), "got: {err}");
}

#[test]
fn list_shows_item_in_text() {
    let stage = stage_dir();
    let notes = stage.join("notes");
    let proj = notes.join("glep-shimeji");
    fs::create_dir_all(&proj).unwrap();
    fs::write(
        proj.join("GLP-0001.md"),
        "---\nstatus: active\ntitle: tray gui\nproject: glep-shimeji\ncreated: 2026-01-01\n---\n\nadd startup toggle\n",
    )
    .unwrap();
    let index_content = "# glep-shimeji\n\n- [[GLP-0001|tray gui]]\n";
    fs::write(proj.join("glep-shimeji.md"), index_content).unwrap();
    let cfg = stage.join("config.json");
    fs::write(
        &cfg,
        format!(
            r#"{{ "notesDir": "{}", "projects": {{ "glep-shimeji": "/repo" }}, "prefixes": {{ "glep-shimeji": "GLP" }} }}"#,
            json_path(&notes)
        ),
    )
    .unwrap();
    let args = parse_args(&[
        "list",
        "--config-path",
        &cfg.to_string_lossy(),
        "--notes-dir",
        &notes.to_string_lossy(),
        "--date",
        "2026-01-01",
    ]);
    let out = run_plain(&args).unwrap();
    assert!(
        out.contains("GLP-0001 :: tray gui"),
        "item line missing: {out}"
    );
}

/// Stages one item in each list section and returns its config and notes paths.
fn list_scopes_fixture() -> (String, String) {
    let stage = stage_dir();
    let notes = stage.join("notes");
    let proj = notes.join("glep-shimeji");
    fs::create_dir_all(&proj).unwrap();
    for (id, title) in [
        ("GLP-0001", "normal"),
        ("GLP-0002", "lowp"),
        ("GLP-0003", "human task"),
        ("GLP-0004", "future task"),
    ] {
        fs::write(
            proj.join(format!("{id}.md")),
            format!("---\nstatus: active\ntitle: {title}\nproject: glep-shimeji\ncreated: 2026-01-01\n---\n\nbody\n"),
        )
        .unwrap();
    }
    fs::write(
        proj.join("glep-shimeji.md"),
        "- [ ] [[GLP-0001|normal]]\n\n## Low-prio\n- [ ] [[GLP-0002|lowp]]\n\n## Human\n- [ ] [[GLP-0003|human task]]\n\n## Future\n- [ ] [[GLP-0004|future task]]\n",
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
    (
        cfg.to_string_lossy().into_owned(),
        notes.to_string_lossy().into_owned(),
    )
}

/// Runs `pwf list` against the scoped-list fixture.
fn list_scopes_run(cfg_s: &str, notes_s: &str, extra: &[&str]) -> String {
    let base = [
        "--config-path",
        cfg_s,
        "--notes-dir",
        notes_s,
        "--date",
        "2026-01-01",
    ];
    let mut argv = vec!["list"];
    argv.extend_from_slice(extra);
    argv.extend_from_slice(&base);
    run_plain(&parse_args(&argv)).unwrap()
}

#[test]
fn list_default_scope_hides_low_prio_human_and_future() {
    let (cfg_s, notes_s) = list_scopes_fixture();
    let def = list_scopes_run(&cfg_s, &notes_s, &[]);
    assert!(def.contains("GLP-0001"), "normal missing: {def}");
    assert!(
        !def.contains("GLP-0002"),
        "Low-prio shown by default: {def}"
    );
    assert!(!def.contains("GLP-0003"), "Human shown by default: {def}");
    assert!(!def.contains("GLP-0004"), "Future shown by default: {def}");
}

#[test]
fn list_human_scope_shows_only_human() {
    let (cfg_s, notes_s) = list_scopes_fixture();
    let h = list_scopes_run(&cfg_s, &notes_s, &["--human"]);
    assert!(!h.contains("GLP-0001"), "normal leaked with --human: {h}");
    assert!(!h.contains("GLP-0002"), "low-prio leaked with --human: {h}");
    assert!(h.contains("GLP-0003"), "Human not shown with --human: {h}");
    assert!(!h.contains("GLP-0004"), "Future leaked with --human: {h}");
}

#[test]
fn list_future_scope_shows_only_future() {
    let (cfg_s, notes_s) = list_scopes_fixture();
    let f = list_scopes_run(&cfg_s, &notes_s, &["--future"]);
    assert!(!f.contains("GLP-0001"), "normal leaked with --future: {f}");
    assert!(
        !f.contains("GLP-0002"),
        "low-prio leaked with --future: {f}"
    );
    assert!(!f.contains("GLP-0003"), "Human leaked with --future: {f}");
    assert!(
        f.contains("GLP-0004"),
        "Future not shown with --future: {f}"
    );
}

#[test]
fn list_all_scope_shows_and_orders_every_section() {
    let (cfg_s, notes_s) = list_scopes_fixture();
    let all = list_scopes_run(&cfg_s, &notes_s, &["--all"]);
    assert!(all.contains("GLP-0001"), "normal missing with --all: {all}");
    assert!(
        all.contains("GLP-0002"),
        "low-prio missing with --all: {all}"
    );
    assert!(all.contains("GLP-0003"), "Human missing with --all: {all}");
    assert!(all.contains("GLP-0004"), "Future missing with --all: {all}");
    let normal_idx = all.find("GLP-0001").unwrap();
    let low_prio_header_idx = all.find("Low-prio").unwrap();
    let low_prio_idx = all.find("GLP-0002").unwrap();
    let human_header_idx = all.find("Human").unwrap();
    let human_idx = all.find("GLP-0003").unwrap();
    let future_header_idx = all.find("Future").unwrap();
    let future_idx = all.find("GLP-0004").unwrap();
    assert!(
        normal_idx < low_prio_header_idx,
        "normal group should lead: {all}"
    );
    assert!(
        low_prio_header_idx < low_prio_idx,
        "Low-prio header misplaced: {all}"
    );
    assert!(
        low_prio_idx < human_header_idx,
        "Human header misplaced: {all}"
    );
    assert!(human_header_idx < human_idx, "Human item misplaced: {all}");
    assert!(
        human_idx < future_header_idx,
        "Future header misplaced: {all}"
    );
    assert!(
        future_header_idx < future_idx,
        "Future item misplaced: {all}"
    );
}

#[test]
fn list_all_shows_human_section_item_in_text() {
    let stage = stage_dir();
    let notes = stage.join("notes");
    let proj = notes.join("glep-shimeji");
    fs::create_dir_all(&proj).unwrap();
    fs::write(
        proj.join("GLP-0001.md"),
        "---\nstatus: active\ntitle: human task\nproject: glep-shimeji\ncreated: 2026-01-01\n---\n\nbody\n",
    )
    .unwrap();
    fs::write(
        proj.join("glep-shimeji.md"),
        "## Human\n- [ ] [[GLP-0001|human task]]\n",
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
    let cfg_s = cfg.to_string_lossy().into_owned();
    let notes_s = notes.to_string_lossy().into_owned();
    let out = run_plain(&parse_args(&[
        "list",
        "--all",
        "--config-path",
        &cfg_s,
        "--notes-dir",
        &notes_s,
        "--date",
        "2026-01-01",
    ]))
    .unwrap();
    assert!(
        out.contains("GLP-0001 :: human task"),
        "item line missing: {out}"
    );
}

#[test]
fn list_all_follows_grouped_order_in_text() {
    let stage = stage_dir();
    let notes = stage.join("notes");
    let cfg_proj = notes.join("config-handler");
    let glp_proj = notes.join("glep-shimeji");
    fs::create_dir_all(&cfg_proj).unwrap();
    fs::create_dir_all(&glp_proj).unwrap();

    for (project_dir, project, id, title) in [
        (&cfg_proj, "config-handler", "CFG-0002", "normal newer"),
        (&cfg_proj, "config-handler", "CFG-0001", "human task"),
        (&glp_proj, "glep-shimeji", "GLP-0002", "low-prio task"),
        (&glp_proj, "glep-shimeji", "GLP-0001", "future task"),
    ] {
        fs::write(
            project_dir.join(format!("{id}.md")),
            format!(
                "---\nstatus: active\ntitle: {title}\nproject: {project}\ncreated: 2026-01-01\n---\n\nbody\n"
            ),
        )
        .unwrap();
    }

    fs::write(
        cfg_proj.join("config-handler.md"),
        "- [ ] [[CFG-0002|normal newer]]\n\n## Human\n- [ ] [[CFG-0001|human task]]\n",
    )
    .unwrap();
    fs::write(
        glp_proj.join("glep-shimeji.md"),
        "## Low-prio\n- [ ] [[GLP-0002|low-prio task]]\n\n## Future\n- [ ] [[GLP-0001|future task]]\n",
    )
    .unwrap();

    let cfg = stage.join("config.json");
    fs::write(
        &cfg,
        format!(
            r#"{{ "notesDir": "{}", "projects": {{ "config-handler": "/repo/cfg", "glep-shimeji": "/repo/glp" }}, "prefixes": {{ "config-handler": "CFG", "glep-shimeji": "GLP" }} }}"#,
            json_path(&notes)
        ),
    )
    .unwrap();
    let cfg_s = cfg.to_string_lossy().into_owned();
    let notes_s = notes.to_string_lossy().into_owned();
    let out = run_plain(&parse_args(&[
        "list",
        "--all",
        "--config-path",
        &cfg_s,
        "--notes-dir",
        &notes_s,
        "--date",
        "2026-01-01",
    ]))
    .unwrap();

    let pos = |id: &str| {
        out.find(id)
            .unwrap_or_else(|| panic!("{id} missing: {out}"))
    };
    assert!(
        pos("CFG-0002") < pos("GLP-0002"),
        "normal should precede low-prio: {out}"
    );
    assert!(
        pos("GLP-0002") < pos("CFG-0001"),
        "low-prio should precede human: {out}"
    );
    assert!(
        pos("CFG-0001") < pos("GLP-0001"),
        "human should precede future: {out}"
    );
}

#[test]
fn list_all_long_keeps_metadata_on_its_own_line_in_every_group() {
    let stage = stage_dir();
    let notes = stage.join("notes");
    let proj = notes.join("glep-shimeji");
    fs::create_dir_all(&proj).unwrap();
    for (id, title) in [
        ("GLP-0001", "normal"),
        ("GLP-0002", "lowp"),
        ("GLP-0003", "human task"),
        ("GLP-0004", "future task"),
    ] {
        fs::write(
            proj.join(format!("{id}.md")),
            format!("---\nstatus: active\ntitle: {title}\nproject: glep-shimeji\ncreated: 2026-01-01\n---\n\nbody\n"),
        )
        .unwrap();
    }
    fs::write(
        proj.join("glep-shimeji.md"),
        "- [ ] [[GLP-0001|normal]]\n\n## Low-prio\n- [ ] [[GLP-0002|lowp]]\n\n## Human\n- [ ] [[GLP-0003|human task]]\n\n## Future\n- [ ] [[GLP-0004|future task]]\n",
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
    let cfg_s = cfg.to_string_lossy().into_owned();
    let notes_s = notes.to_string_lossy().into_owned();
    let out = run_plain(&parse_args(&[
        "list",
        "--all",
        "--long",
        "--config-path",
        &cfg_s,
        "--notes-dir",
        &notes_s,
        "--date",
        "2026-01-01",
    ]))
    .unwrap();

    for expected in [
        "GLP-0001 :: normal\n  status:",
        "GLP-0002 :: lowp\n  status:",
        "GLP-0003 :: human task\n  status:",
        "GLP-0004 :: future task\n  status:",
    ] {
        assert!(
            out.contains(expected),
            "expected long metadata on its own line: {expected}\n\n{out}"
        );
    }
}

#[test]
fn route_project_shortcut_uses_list_scopes() {
    let stage = stage_dir();
    let notes = stage.join("notes");
    let proj = notes.join("config-handler");
    fs::create_dir_all(&proj).unwrap();
    for (id, title) in [
        ("CFG-0001", "normal"),
        ("CFG-0002", "lowp"),
        ("CFG-0003", "human task"),
        ("CFG-0004", "future task"),
    ] {
        fs::write(
            proj.join(format!("{id}.md")),
            format!("---\nstatus: active\ntitle: {title}\nproject: config-handler\ncreated: 2026-01-01\n---\n\nbody\n"),
        )
        .unwrap();
    }
    fs::write(
        proj.join("config-handler.md"),
        "- [ ] [[CFG-0001|normal]]\n\n## Low-prio\n- [ ] [[CFG-0002|lowp]]\n\n## Human\n- [ ] [[CFG-0003|human task]]\n\n## Future\n- [ ] [[CFG-0004|future task]]\n",
    )
    .unwrap();
    let cfg = stage.join("config.json");
    fs::write(
        &cfg,
        format!(
            r#"{{ "notesDir": "{}", "projects": {{ "config-handler": "/repo" }}, "prefixes": {{ "config-handler": "CFG" }} }}"#,
            json_path(&notes)
        ),
    )
    .unwrap();
    let cfg_s = cfg.to_string_lossy().into_owned();
    let notes_s = notes.to_string_lossy().into_owned();
    let run = |extra: &[&str]| {
        let mut argv = vec!["cfg"];
        argv.extend_from_slice(extra);
        argv.extend_from_slice(&[
            "--config-path",
            &cfg_s,
            "--notes-dir",
            &notes_s,
            "--date",
            "2026-01-01",
        ]);
        run_plain(&parse_args(&argv)).unwrap()
    };

    let def = run(&[]);
    assert!(def.contains("CFG-0001"), "normal missing: {def}");
    assert!(!def.contains("CFG-0002"), "low-prio leaked: {def}");
    assert!(!def.contains("CFG-0003"), "human leaked: {def}");
    assert!(!def.contains("CFG-0004"), "future leaked: {def}");

    let human = run(&["--human"]);
    assert!(!human.contains("CFG-0001"), "normal leaked: {human}");
    assert!(!human.contains("CFG-0002"), "low-prio leaked: {human}");
    assert!(human.contains("CFG-0003"), "human missing: {human}");
    assert!(!human.contains("CFG-0004"), "future leaked: {human}");

    let all = run(&["--all"]);
    assert!(all.contains("CFG-0001"), "normal missing: {all}");
    assert!(all.contains("CFG-0002"), "low-prio missing: {all}");
    assert!(all.contains("CFG-0003"), "human missing: {all}");
    assert!(all.contains("CFG-0004"), "future missing: {all}");
}

#[test]
fn list_long_shows_per_item_metadata() {
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
        "# glep-shimeji\n\n- [[GLP-0001|tray gui]]\n",
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
    let args = parse_args(&[
        "list",
        "--long",
        "--config-path",
        &cfg.to_string_lossy(),
        "--notes-dir",
        &notes.to_string_lossy(),
        "--date",
        "2026-01-01",
    ]);
    let out = run_plain(&args).unwrap();
    assert!(out.contains("GLP-0001 :: tray gui"), "got: {out}");
    assert!(out.contains("  status: active"), "got: {out}");
    assert!(out.contains("  launch: READY"), "got: {out}");
    assert!(out.contains("  repo: /repo"), "got: {out}");
    assert!(out.contains("  prompt: add startup toggle"), "got: {out}");
}

/// Stages `count` open items and returns the config and notes paths.
fn stage_many(count: usize) -> (std::path::PathBuf, std::path::PathBuf) {
    let stage = stage_dir();
    let notes = stage.join("notes");
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
        let _ = writeln!(index, "- [ ] [[{id}|t{n}]]");
    }
    fs::write(proj.join("glep-shimeji.md"), index).unwrap();
    let cfg = stage.join("config.json");
    fs::write(
        &cfg,
        format!(
            r#"{{ "notesDir": "{}", "projects": {{ "glep-shimeji": "/repo" }}, "prefixes": {{ "glep-shimeji": "GLP" }} }}"#,
            json_path(&notes)
        ),
    )
    .unwrap();
    (cfg, notes)
}

fn list_run(cfg: &std::path::Path, notes: &std::path::Path, extra: &[&str]) -> String {
    let cfg_s = cfg.to_string_lossy().into_owned();
    let notes_s = notes.to_string_lossy().into_owned();
    let mut argv = vec!["list"];
    argv.extend_from_slice(extra);
    argv.extend_from_slice(&[
        "--config-path",
        &cfg_s,
        "--notes-dir",
        &notes_s,
        "--date",
        "2026-01-01",
    ]);
    run_plain(&parse_args(&argv)).unwrap()
}

/// Stages two projects whose equal creation dates force the full-ID tiebreak.
fn stage_two_projects_same_created_date() -> (std::path::PathBuf, std::path::PathBuf) {
    let stage = stage_dir();
    let notes = stage.join("notes");
    for (project, id, title) in [
        ("config-handler", "CFG-0001", "cfg older"),
        ("config-handler", "CFG-0002", "cfg newer"),
        ("pwf", "PWF-9999", "pwf newest"),
    ] {
        let proj = notes.join(project);
        fs::create_dir_all(&proj).unwrap();
        fs::write(
            proj.join(format!("{id}.md")),
            format!("---\nstatus: active\ntitle: {title}\nproject: {project}\ncreated: 2026-01-01\n---\n\nbody\n"),
        )
        .unwrap();
    }
    fs::write(
        notes.join("config-handler/config-handler.md"),
        "# config-handler\n\n- [ ] [[CFG-0001|cfg older]]\n- [ ] [[CFG-0002|cfg newer]]\n",
    )
    .unwrap();
    fs::write(
        notes.join("pwf/pwf.md"),
        "# pwf\n\n- [ ] [[PWF-9999|pwf newest]]\n",
    )
    .unwrap();
    let cfg = stage.join("config.json");
    fs::write(
        &cfg,
        format!(
            r#"{{ "notesDir": "{}", "projects": {{ "config-handler": "/repo/cfg", "pwf": "/repo/pwf" }}, "prefixes": {{ "config-handler": "CFG", "pwf": "PWF" }} }}"#,
            json_path(&notes)
        ),
    )
    .unwrap();
    (cfg, notes)
}

#[test]
fn list_default_is_flat_across_projects() {
    let (cfg, notes) = stage_two_projects_same_created_date();

    let out = list_run(&cfg, &notes, &["-n", "0"]);

    assert_eq!(
        out.lines().map(str::to_string).collect::<Vec<_>>(),
        [
            "PWF-9999 :: pwf newest",
            "CFG-0002 :: cfg newer",
            "CFG-0001 :: cfg older",
        ]
    );
}

#[test]
fn list_order_project_id_reproduces_legacy_grouped_ordering() {
    let (cfg, notes) = stage_two_projects_same_created_date();

    let out = list_run(&cfg, &notes, &["-n", "0", "--order", "project-id"]);

    assert_eq!(
        out.lines().map(str::to_string).collect::<Vec<_>>(),
        [
            "CFG-0002 :: cfg newer",
            "CFG-0001 :: cfg older",
            "PWF-9999 :: pwf newest",
        ]
    );
}

#[test]
fn list_caps_to_default_ten_and_signals_more() {
    let (cfg, notes) = stage_many(12);
    let out = list_run(&cfg, &notes, &[]);
    assert!(out.contains("GLP-0012"), "newest missing: {out}");
    assert!(out.contains("GLP-0003"), "10th newest missing: {out}");
    assert!(!out.contains("GLP-0002"), "11th item leaked: {out}");
    assert!(!out.contains("GLP-0001"), "12th item leaked: {out}");
    assert!(out.contains("2 more"), "more footer missing: {out}");
    assert!(out.contains("-n 0"), "escape hatch missing: {out}");
}

#[test]
fn list_n_zero_shows_all_no_footer() {
    let (cfg, notes) = stage_many(12);
    let out = list_run(&cfg, &notes, &["-n", "0"]);
    for n in 1..=12 {
        assert!(
            out.contains(&format!("GLP-{n:04}")),
            "GLP-{n:04} missing: {out}"
        );
    }
    assert!(!out.contains("more"), "footer shown with -n 0: {out}");
}

#[test]
fn list_n_explicit_caps() {
    let (cfg, notes) = stage_many(12);
    let out = list_run(&cfg, &notes, &["-n", "3"]);
    assert!(out.contains("GLP-0012"), "newest missing: {out}");
    assert!(out.contains("GLP-0011"), "2nd newest missing: {out}");
    assert!(out.contains("GLP-0010"), "3rd newest missing: {out}");
    assert!(!out.contains("GLP-0009"), "4th item leaked: {out}");
}

#[test]
fn list_is_capped_and_ordered() {
    let (cfg, notes) = stage_many(12);
    let out = list_run(&cfg, &notes, &[]);
    assert!(out.contains("GLP-0012"), "newest missing: {out}");
    assert!(out.contains("GLP-0003"), "10th item missing: {out}");
    assert!(!out.contains("GLP-0002"), "11th item leaked: {out}");
    assert!(!out.contains("GLP-0001"), "12th item leaked: {out}");
    assert!(out.contains("2 more"), "hidden-count footer missing: {out}");
    assert!(out.contains("-n 0"), "escape hatch missing: {out}");
    assert!(
        out.find("GLP-0012").unwrap() < out.find("GLP-0003").unwrap(),
        "not newest-first: {out}"
    );
}

#[test]
fn list_long_shows_prereq_status() {
    let stage = stage_dir();
    let notes = stage.join("notes");
    let proj = notes.join("config-handler");
    fs::create_dir_all(&proj).unwrap();
    fs::write(
        proj.join("CFG-0014.md"),
        "---\nstatus: done\ntitle: prereq\nproject: config-handler\ncreated: 2026-01-01\n---\n\nbody\n",
    )
    .unwrap();
    fs::write(
        proj.join("CFG-0015.md"),
        "---\nstatus: active\ntitle: active prereq\nproject: config-handler\ncreated: 2026-01-01\n---\n\nbody\n",
    )
    .unwrap();
    fs::write(
        proj.join("CFG-0020.md"),
        "---\nstatus: active\ntitle: dependent\nproject: config-handler\ncreated: 2026-01-01\nprereq: \"[[CFG-0014]], [[CFG-0015]], [[CFG-9999]]\"\n---\n\ndo the dependent thing\n",
    )
    .unwrap();
    fs::write(
        proj.join("config-handler.md"),
        "- [ ] [[CFG-0020|dependent]]\n",
    )
    .unwrap();
    let cfg = stage.join("config.json");
    fs::write(
        &cfg,
        format!(
            r#"{{ "notesDir": "{}", "projects": {{ "config-handler": "/repo" }}, "prefixes": {{ "config-handler": "CFG" }} }}"#,
            json_path(&notes)
        ),
    )
    .unwrap();
    let args = parse_args(&[
        "list",
        "--long",
        "--config-path",
        &cfg.to_string_lossy(),
        "--notes-dir",
        &notes.to_string_lossy(),
        "--date",
        "2026-01-01",
    ]);
    let out = run_plain(&args).unwrap();
    assert!(
        out.contains("prereq: CFG-0014 (done), CFG-0015 (active), CFG-9999 (missing)"),
        "got: {out}"
    );
}

#[test]
fn done_keeps_done_link_in_index_in_place_without_bak() {
    let stage = stage_dir();
    let notes = stage.join("notes");
    let proj = notes.join("glep-shimeji");
    fs::create_dir_all(&proj).unwrap();
    let item_content = "---\nstatus: active\ntitle: tray gui\nproject: glep-shimeji\ncreated: 2026-01-01\n---\n\nadd startup toggle\n";
    fs::write(proj.join("GLP-0001.md"), item_content).unwrap();
    let index_content = "# glep-shimeji\n\n- [[GLP-0001|tray gui]]\n\n## Later\n";
    fs::write(proj.join("glep-shimeji.md"), index_content).unwrap();
    let cfg = stage.join("config.json");
    fs::write(
        &cfg,
        format!(
            r#"{{ "notesDir": "{}", "projects": {{ "glep-shimeji": "/repo" }}, "prefixes": {{ "glep-shimeji": "GLP" }} }}"#,
            json_path(&notes)
        ),
    )
    .unwrap();
    let args = parse_args(&[
        "done",
        "--id",
        "GLP-0001",
        "--config-path",
        &cfg.to_string_lossy(),
        "--notes-dir",
        &notes.to_string_lossy(),
        "--date",
        "2026-01-01",
    ]);
    let out = run_plain(&args).unwrap();
    assert!(out.starts_with("Done GLP-0001"), "got: {out}");
    let item = fs::read_to_string(proj.join("GLP-0001.md")).unwrap();
    assert!(item.contains("status: done"));
    assert!(item.contains("completed: 2026-01-01"));
    let index = fs::read_to_string(proj.join("glep-shimeji.md")).unwrap();
    assert_eq!(
        index,
        "---\nid: glp\ntitle: glep-shimeji\n---\n\n# glep-shimeji\n\n- [x] [[GLP-0001]] ✅ 2026-01-01\n\n## Later\n"
    );
    // Writes must not leave backup files in the git-tracked vault.
    assert!(!proj.join("GLP-0001.md.bak").exists());
    assert!(!proj.join("glep-shimeji.md.bak").exists());
}

#[test]
fn done_evicts_oldest_link_but_keeps_note_in_project_dir() {
    let stage = stage_dir();
    let notes = stage.join("notes");
    let proj = notes.join("glep-shimeji");
    fs::create_dir_all(&proj).unwrap();
    let mut index = String::new();
    for n in 1..=6 {
        let id = format!("GLP-{n:04}");
        fs::write(
            proj.join(format!("{id}.md")),
            format!("---\nstatus: done\ncompleted: 2026-01-{n:02}\ntitle: t{n}\nproject: glep-shimeji\ncreated: 2026-01-{n:02}\n---\n\nbody\n"),
        )
        .unwrap();
        let _ = writeln!(index, "- [x] [[{id}]] ✅ 2026-01-{n:02}");
    }
    fs::write(
        proj.join("GLP-0007.md"),
        "---\nstatus: active\ntitle: seven\nproject: glep-shimeji\ncreated: 2026-06-13\n---\n\nbody\n",
    )
    .unwrap();
    index.push_str("- [ ] [[GLP-0007]]\n");
    fs::write(proj.join("glep-shimeji.md"), &index).unwrap();
    let cfg = stage.join("config.json");
    fs::write(
        &cfg,
        format!(
            r#"{{ "notesDir": "{}", "projects": {{ "glep-shimeji": "/repo" }}, "prefixes": {{ "glep-shimeji": "GLP" }} }}"#,
            json_path(&notes)
        ),
    )
    .unwrap();
    let args = parse_args(&[
        "done",
        "--id",
        "GLP-0007",
        "--config-path",
        &cfg.to_string_lossy(),
        "--notes-dir",
        &notes.to_string_lossy(),
        "--date",
        "2026-06-13",
    ]);
    run_plain(&args).unwrap();
    let index = fs::read_to_string(proj.join("glep-shimeji.md")).unwrap();
    assert!(!index.contains("GLP-0001"), "oldest unlinked: {index}");
    assert!(index.contains("- [x] [[GLP-0007]] ✅ 2026-06-13"));
    assert_eq!(index.matches("- [x]").count(), 6);
    // Queue eviction changes index visibility but leaves the authoritative note in place.
    assert!(
        proj.join("GLP-0001.md").exists(),
        "evicted note stays in project dir"
    );
    assert!(
        !proj.join("_archive").exists(),
        "archive directory is obsolete"
    );
}

/// Stages one active item for update tests.
fn stage_update_item(body: &str) -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
    let stage = stage_dir();
    let notes = stage.join("notes");
    let proj = notes.join("glep-shimeji");
    fs::create_dir_all(&proj).unwrap();
    fs::write(
        proj.join("GLP-0001.md"),
        format!("---\nstatus: active\ntitle: tray gui\nproject: glep-shimeji\ncreated: 2026-01-01\n---\n\n{body}\n"),
    )
    .unwrap();
    fs::write(proj.join("glep-shimeji.md"), "- [ ] [[GLP-0001]]\n").unwrap();
    let cfg = stage.join("config.json");
    fs::write(
        &cfg,
        format!(
            r#"{{ "notesDir": "{}", "projects": {{ "glep-shimeji": "/repo" }}, "prefixes": {{ "glep-shimeji": "GLP" }} }}"#,
            json_path(&notes)
        ),
    )
    .unwrap();
    (notes, proj, cfg)
}

#[test]
fn update_rewrites_body_via_note_body_and_preserves_frontmatter() {
    let (notes, proj, cfg) = stage_update_item("old prompt");
    let args = parse_args(&[
        "update",
        "--id",
        "GLP-0001",
        "--prompt",
        "new prompt / second goal",
        "--config-path",
        &cfg.to_string_lossy(),
        "--notes-dir",
        &notes.to_string_lossy(),
        "--date",
        "2026-01-01",
    ]);
    let out = run_plain(&args).unwrap();
    assert!(
        out.contains("Updated pwf task: **GLP-0001") && out.contains("glep-shimeji :: tray gui"),
        "expected update confirmation for GLP-0001: {out}"
    );

    let item = fs::read_to_string(proj.join("GLP-0001.md")).unwrap();
    assert!(
        item.contains("## Goals\n- new prompt\n- second goal"),
        "body should be note_body-wrapped: {item}"
    );
    assert!(!item.contains("old prompt"), "old body replaced: {item}");
    assert!(item.contains("status: active"));
    assert!(item.contains("title: tray gui"));
    assert!(item.contains("project: glep-shimeji"));
    assert!(item.contains("created: 2026-01-01"));
    assert!(!item.contains("completed:"), "no completed added: {item}");
    assert!(!proj.join("GLP-0001.md.bak").exists());
}

#[test]
fn update_title_only_leaves_body_untouched() {
    let (notes, proj, cfg) = stage_update_item("## Goals\n- keep me");
    let args = parse_args(&[
        "update",
        "--id",
        "GLP-0001",
        "--title",
        "new title",
        "--config-path",
        &cfg.to_string_lossy(),
        "--notes-dir",
        &notes.to_string_lossy(),
    ]);
    run_plain(&args).unwrap();
    let item = fs::read_to_string(proj.join("GLP-0001.md")).unwrap();
    assert!(item.contains("title: new title"), "title replaced: {item}");
    assert!(!item.contains("title: tray gui"));
    assert!(
        item.contains("## Goals\n- keep me"),
        "body untouched: {item}"
    );
}

#[test]
fn update_prompt_only_leaves_title_untouched() {
    let (notes, proj, cfg) = stage_update_item("old body");
    let args = parse_args(&[
        "update",
        "--id",
        "GLP-0001",
        "--prompt",
        "fresh prompt",
        "--config-path",
        &cfg.to_string_lossy(),
        "--notes-dir",
        &notes.to_string_lossy(),
    ]);
    run_plain(&args).unwrap();
    let item = fs::read_to_string(proj.join("GLP-0001.md")).unwrap();
    assert!(item.contains("title: tray gui"), "title untouched: {item}");
    assert!(item.contains("## Goals\n- fresh prompt"));
}

#[test]
fn update_keeps_placeholder_prompt_raw_so_it_stays_detectable() {
    let (notes, proj, cfg) = stage_update_item("## Goals\n- old");
    let args = parse_args(&[
        "update",
        "--id",
        "GLP-0001",
        "--prompt",
        "TODO",
        "--config-path",
        &cfg.to_string_lossy(),
        "--notes-dir",
        &notes.to_string_lossy(),
    ]);
    run_plain(&args).unwrap();
    let item = fs::read_to_string(proj.join("GLP-0001.md")).unwrap();
    // Preserve raw placeholders so `is_placeholder_prompt` can recognize them.
    assert!(!item.contains("## Goals"), "placeholder stored raw: {item}");
    assert!(item.trim_end().ends_with("TODO"), "raw TODO body: {item}");
}

#[test]
fn update_requires_at_least_one_field() {
    let (notes, _proj, cfg) = stage_update_item("body");
    let args = parse_args(&[
        "update",
        "--id",
        "GLP-0001",
        "--config-path",
        &cfg.to_string_lossy(),
        "--notes-dir",
        &notes.to_string_lossy(),
    ]);
    let err = run_plain(&args).unwrap_err();
    assert!(err.contains("nothing to update"), "got: {err}");
}

#[test]
fn update_unknown_id_errors() {
    let (notes, _proj, cfg) = stage_update_item("body");
    let args = parse_args(&[
        "update",
        "--id",
        "GLP-9999",
        "--prompt",
        "x",
        "--config-path",
        &cfg.to_string_lossy(),
        "--notes-dir",
        &notes.to_string_lossy(),
    ]);
    assert!(run_plain(&args).is_err());
}

#[test]
fn done_with_report_appends_report_section() {
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
    let args = parse_args(&[
        "done",
        "--id",
        "GLP-0001",
        "--report",
        "  launch prompt now tells agents to add reports\nwhen no plan covers the task  ",
        "--config-path",
        &cfg.to_string_lossy(),
        "--notes-dir",
        &notes.to_string_lossy(),
        "--date",
        "2026-01-01",
    ]);
    run_plain(&args).unwrap();
    let item = fs::read_to_string(proj.join("GLP-0001.md")).unwrap();
    assert!(
        item.contains("status: done\ncompleted: 2026-01-01\n"),
        "got: {item}"
    );
    assert!(
        item.ends_with(
            "\n### Report\n\nlaunch prompt now tells agents to add reports when no plan covers the task\n"
        ),
        "got: {item}"
    );
}

#[test]
fn cancel_requires_report() {
    let stage = stage_dir();
    let notes = stage.join("notes");
    let proj = notes.join("glep-shimeji");
    fs::create_dir_all(&proj).unwrap();
    fs::write(
        proj.join("GLP-0001.md"),
        "---\nstatus: active\ntitle: tray gui\nproject: glep-shimeji\ncreated: 2026-01-01\n---\n\nadd startup toggle\n",
    )
    .unwrap();
    fs::write(proj.join("glep-shimeji.md"), "- [ ] [[GLP-0001]]\n").unwrap();
    let cfg = stage.join("config.json");
    fs::write(
        &cfg,
        format!(
            r#"{{ "notesDir": "{}", "projects": {{ "glep-shimeji": "/repo" }}, "prefixes": {{ "glep-shimeji": "GLP" }} }}"#,
            json_path(&notes)
        ),
    )
    .unwrap();
    let args = parse_args(&[
        "cancel",
        "--id",
        "GLP-0001",
        "--config-path",
        &cfg.to_string_lossy(),
        "--notes-dir",
        &notes.to_string_lossy(),
    ]);

    let err = run_plain(&args).unwrap_err();

    assert_eq!(err, "--report is required for cancel.");
}

#[test]
fn cancel_with_report_marks_item_cancelled_and_rotates_done_queue() {
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
        "# glep-shimeji\n\n- [ ] [[GLP-0001|tray gui]]\n",
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
    let args = parse_args(&[
        "cancel",
        "--id",
        "GLP-0001",
        "--report",
        "  tried the implementation\nblocked by upstream scope  ",
        "--config-path",
        &cfg.to_string_lossy(),
        "--notes-dir",
        &notes.to_string_lossy(),
        "--date",
        "2026-01-01",
    ]);

    let out = run_plain(&args).unwrap();

    assert!(out.starts_with("Cancelled GLP-0001"), "got: {out}");
    let item = fs::read_to_string(proj.join("GLP-0001.md")).unwrap();
    assert!(
        item.contains("status: cancelled\ncompleted: 2026-01-01\n"),
        "got: {item}"
    );
    assert!(
        item.ends_with("\n### Report\n\ntried the implementation blocked by upstream scope\n"),
        "got: {item}"
    );
    let index = fs::read_to_string(proj.join("glep-shimeji.md")).unwrap();
    assert_eq!(
        index,
        "---\nid: glp\ntitle: glep-shimeji\n---\n\n# glep-shimeji\n\n- [x] [[GLP-0001]] ✅ 2026-01-01\n"
    );
}

fn remove_stage() -> (
    std::path::PathBuf,
    std::path::PathBuf,
    std::path::PathBuf,
    std::path::PathBuf,
) {
    let stage = stage_dir();
    let notes = stage.join("notes");
    let repo = stage.join("repo");
    let proj = notes.join("pwf");
    fs::create_dir_all(&proj).unwrap();
    fs::create_dir_all(&repo).unwrap();
    fs::write(
        proj.join("PWF-0001.md"),
        "---\nstatus: active\ntitle: stale task\nproject: pwf\ncreated: 2026-01-01\n---\n\nremove me\n",
    )
    .unwrap();
    fs::write(
        proj.join("pwf.md"),
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
    (notes, repo, proj, cfg)
}

#[test]
fn remove_deletes_pwf_item_file_and_index_link_with_id_only() {
    let (notes, _repo, proj, cfg) = remove_stage();
    let args = parse_args(&[
        "remove",
        "--id",
        "pwf-0001",
        "--config-path",
        &cfg.to_string_lossy(),
        "--notes-dir",
        &notes.to_string_lossy(),
    ]);
    let out = run_plain(&args).unwrap();
    assert!(
        out.contains("Removed pwf task: **PWF-0001") && out.contains("pwf :: stale task"),
        "expected remove confirmation for PWF-0001: {out}"
    );
    assert!(!proj.join("PWF-0001.md").exists());
    let index = fs::read_to_string(proj.join("pwf.md")).unwrap();
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
fn add_continue_handoff_builds_handoff_prompt() {
    let stage = stage_dir();
    let notes = stage.join("notes");
    let repo = stage.join("repo");
    let proj = notes.join("glep-shimeji");
    let handoff_dir = repo.join("docs").join("handoffs");
    fs::create_dir_all(&proj).unwrap();
    fs::create_dir_all(&handoff_dir).unwrap();
    fs::write(proj.join("glep-shimeji.md"), "# glep-shimeji\n").unwrap();
    fs::write(
        handoff_dir.join("2026-01-01-api-cleanup.md"),
        "# API cleanup handoff\n",
    )
    .unwrap();
    let cfg = stage.join("config.json");
    fs::write(
        &cfg,
        format!(
            r#"{{ "notesDir": "{}", "projects": {{ "glep-shimeji": "{}" }}, "prefixes": {{ "glep-shimeji": "GLP" }} }}"#,
            json_path(&notes),
            json_path(&repo)
        ),
    )
    .unwrap();
    let args = parse_args(&[
        "add",
        "glep-shimeji",
        "--continue-handoff",
        "--config-path",
        &cfg.to_string_lossy(),
        "--notes-dir",
        &notes.to_string_lossy(),
        "--date",
        "2026-01-01",
    ]);
    let out = run_plain(&args).unwrap();
    assert!(out.starts_with("Added pwf task: **GLP-0001"), "got: {out}");
    assert!(
        out.contains(":: continue api cleanup"),
        "title not in output: {out}"
    );
    let item = fs::read_to_string(proj.join("GLP-0001.md")).unwrap();
    assert!(
        item.contains("Continue the handoff at @docs/handoffs/2026-01-01-api-cleanup.md."),
        "prompt not in item: {item}"
    );
}

#[test]
fn add_with_title_flag_accepts_prereq() {
    let stage = stage_dir();
    let notes = stage.join("notes");
    let proj = notes.join("glep-shimeji");
    fs::create_dir_all(&proj).unwrap();
    fs::write(
        proj.join("GLP-0001.md"),
        "---\nstatus: active\ntitle: prerequisite\nproject: glep-shimeji\ncreated: 2026-01-01\n---\n\nbody\n",
    )
    .unwrap();
    fs::write(
        proj.join("glep-shimeji.md"),
        "- [ ] [[GLP-0001|prerequisite]]\n",
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
    let args = parse_args(&[
        "add",
        "glep-shimeji",
        "do",
        "dependent",
        "work",
        "--title",
        "dependent",
        "--prereq",
        "GLP-0001",
        "--config-path",
        &cfg.to_string_lossy(),
        "--notes-dir",
        &notes.to_string_lossy(),
        "--date",
        "2026-01-01",
    ]);
    run_plain(&args).unwrap();
    let item = fs::read_to_string(proj.join("GLP-0002.md")).unwrap();
    assert!(
        item.contains("prereq: \"[[GLP-0001]]\"\n---"),
        "got: {item}"
    );
}

#[test]
fn route_create_verbs_error_with_add_hint() {
    let stage = stage_dir();
    let notes = stage.join("notes");
    fs::create_dir_all(&notes).unwrap();
    let cfg = stage.join("config.json");
    fs::write(
        &cfg,
        format!(
            r#"{{ "notesDir": "{}", "projects": {{ "glep-shimeji": "/repo" }}, "prefixes": {{ "glep-shimeji": "GLP" }} }}"#,
            json_path(&notes)
        ),
    )
    .unwrap();
    let cfg_s = cfg.to_string_lossy();
    let notes_s = notes.to_string_lossy();

    for (words, expected) in [
        (&["add"][..], r#"Use: pwf add <project> "<prompt>""#),
        (
            &["add-titled", "glep-shimeji"][..],
            r#"Use: pwf add <project> "<prompt>""#,
        ),
    ] {
        let mut argv = vec!["route", "--config-path", &cfg_s, "--notes-dir", &notes_s];
        argv.extend(words);
        let parsed = parse_args(&argv);
        let err = run_plain(&parsed).unwrap_err();
        assert_eq!(err, expected);
    }
}

fn show_stage() -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
    let stage = stage_dir();
    let notes = stage.join("notes");
    let proj = notes.join("glep-shimeji");
    fs::create_dir_all(&proj).unwrap();
    fs::write(
        proj.join("GLP-0001.md"),
        "---\nid: GLP-0001\nstatus: active\ntitle: tray gui\nproject: glep-shimeji\ncreated: 2026-01-01\n---\n\nadd startup toggle\n",
    )
    .unwrap();
    fs::write(proj.join("glep-shimeji.md"), "- [[GLP-0001|tray gui]]\n").unwrap();
    let cfg = stage.join("config.json");
    fs::write(
        &cfg,
        format!(
            r#"{{ "notesDir": "{}", "projects": {{ "glep-shimeji": "/repo" }}, "prefixes": {{ "glep-shimeji": "GLP" }} }}"#,
            json_path(&notes)
        ),
    )
    .unwrap();
    (notes, proj, cfg)
}

fn show_path_args(
    cfg: &std::path::Path,
    notes: &std::path::Path,
    extra: &[&str],
) -> pending_work::Command {
    let cfg_s = cfg.to_string_lossy();
    let notes_s = notes.to_string_lossy();
    let mut argv = vec![
        "show",
        "--path",
        "--id",
        "GLP-0001",
        "--config-path",
        &cfg_s,
        "--notes-dir",
        &notes_s,
    ];
    argv.extend_from_slice(extra);
    parse_args(&argv)
}

#[test]
fn show_path_prints_item_note_path_plain() {
    let (notes, proj, cfg) = show_stage();
    let out = run_plain(&show_path_args(&cfg, &notes, &[])).unwrap();
    assert_eq!(
        out.trim(),
        proj.join("GLP-0001.md").to_string_lossy().as_ref()
    );
}

#[test]
fn show_markdown_preserves_item_note_bytes() {
    let (notes, proj, cfg) = show_stage();
    let expected = fs::read_to_string(proj.join("GLP-0001.md")).unwrap();
    let cfg_s = cfg.to_string_lossy();
    let notes_s = notes.to_string_lossy();
    let args = parse_args(&[
        "show",
        "--id",
        "GLP-0001",
        "--config-path",
        &cfg_s,
        "--notes-dir",
        &notes_s,
    ]);

    assert_eq!(run_plain(&args).unwrap(), expected);
}

#[test]
fn show_path_succeeds_when_item_note_is_missing() {
    let (notes, proj, cfg) = show_stage();
    fs::remove_file(proj.join("GLP-0001.md")).unwrap();

    let out = run_plain(&show_path_args(&cfg, &notes, &[])).unwrap();

    assert_eq!(
        out.trim(),
        proj.join("GLP-0001.md").to_string_lossy().as_ref()
    );
}

#[test]
fn show_unknown_id_errors() {
    let (notes, _proj, cfg) = show_stage();
    let cfg = cfg.to_string_lossy();
    let notes = notes.to_string_lossy();
    let args = parse_args(&[
        "show",
        "--path",
        "--id",
        "GLP-0099",
        "--config-path",
        &cfg,
        "--notes-dir",
        &notes,
    ]);
    let err = run_plain(&args).unwrap_err();
    assert!(err.contains("GLP-0099"), "error should name the id: {err}");
}

#[test]
fn show_requires_id() {
    let (notes, _proj, cfg) = show_stage();
    let cfg = cfg.to_string_lossy();
    let notes = notes.to_string_lossy();
    let args = parse_args(&[
        "show",
        "--path",
        "--config-path",
        &cfg,
        "--notes-dir",
        &notes,
    ]);
    let err = run_plain(&args).unwrap_err();
    assert!(err.contains("--id"), "error should mention --id: {err}");
}

// Agent verification rendering is covered where the crate-internal launcher seam is accessible.

fn stage_dir() -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("pwstage_{}", nanos()));
    fs::create_dir_all(&d).unwrap();
    d
}

fn parse_args(argv: &[&str]) -> pending_work::Command {
    migrate_test_fixture(argv);
    let v = argv.iter().map(std::string::ToString::to_string).collect();
    let pwf::command::Engine::PendingWork(command) = pwf::command::parse_argv(v).unwrap().engine
    else {
        panic!("expected pending-work command");
    };
    command
}

fn migrate_test_fixture(argv: &[&str]) {
    let Some(notes_dir) = flag_value(argv, "--notes-dir") else {
        return;
    };
    let Some(config_path) = flag_value(argv, "--config-path") else {
        return;
    };
    let Ok(config) = fs::read_to_string(config_path) else {
        return;
    };
    let Ok(config) = serde_json::from_str::<serde_json::Value>(&config) else {
        return;
    };
    let Some(projects) = config
        .get("projects")
        .and_then(serde_json::Value::as_object)
    else {
        return;
    };
    let prefixes = config
        .get("prefixes")
        .and_then(serde_json::Value::as_object);
    for project in projects.keys() {
        let Some(prefix) = prefixes
            .and_then(|prefixes| prefixes.get(project))
            .and_then(serde_json::Value::as_str)
        else {
            continue;
        };
        migrate_test_project(std::path::Path::new(notes_dir), project, prefix);
    }
}

fn flag_value<'args>(argv: &'args [&str], flag: &str) -> Option<&'args str> {
    argv.windows(2)
        .find(|pair| pair[0] == flag)
        .map(|pair| pair[1])
}

fn migrate_test_project(notes_dir: &std::path::Path, project: &str, prefix: &str) {
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
        return;
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

/// Escapes a filesystem path for JSON, including Windows backslashes.
fn json_path(p: &std::path::Path) -> String {
    p.to_string_lossy().replace('\\', "\\\\")
}

fn nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}
