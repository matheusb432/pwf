use std::fs;

use pwf::{
    confirm::FakeConfirm,
    engines::{clean, pending_work as pwk},
};

// ── Task 12 ──────────────────────────────────────────────────────────────────

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
    let out = pwk::run_args(&args).unwrap();
    assert!(out.starts_with("ADDED PWF TASK [GLP-0001]"), "got: {out}");
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
fn add_skips_ids_already_present_in_archive() {
    const PROJECT: &str = "config-handler";
    const ARCHIVE_DIR: &str = "_archive";

    let stage = stage_dir();
    let notes = stage.join("notes");
    let proj = notes.join(PROJECT);
    let archive = proj.join(ARCHIVE_DIR);
    fs::create_dir_all(&archive).unwrap();
    fs::write(
        proj.join("CFG-0088.md"),
        format!(
            "---\nstatus: active\ntitle: active\nproject: {PROJECT}\ncreated: 2026-01-01\n---\n\nbody\n"
        ),
    )
    .unwrap();
    fs::write(
        archive.join("CFG-0089.md"),
        format!(
            "---\nstatus: done\ntitle: archived\nproject: {PROJECT}\ncreated: 2026-01-01\n---\n\nbody\n"
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
        "avoid archived duplicate",
        "--config-path",
        &cfg.to_string_lossy(),
        "--notes-dir",
        &notes.to_string_lossy(),
        "--date",
        "2026-01-01",
    ]);

    let out = pwk::run_args(&args).unwrap();
    assert!(out.starts_with("ADDED PWF TASK [CFG-0090]"), "got: {out}");
    assert!(proj.join("CFG-0090.md").exists());
    assert!(archive.join("CFG-0089.md").exists());
}

#[test]
fn add_caps_inferred_title_for_long_prompt_without_ampersand() {
    // CFG-0075 regression, end-to-end: a long prompt with no '&' marker and no
    // explicit --title must not write the whole prompt as the frontmatter title.
    // Keep this on the real add path with a realistic long prompt; the failure is
    // visible only after title inference and note writing meet.
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
    pwk::run_args(&args).unwrap();

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
    // The full prompt is still preserved verbatim in the Goals body.
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
    // An existing GLP-0001 so the new item allocates GLP-0002 under ## Human.
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
    pwk::run_args(&args).unwrap();
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
    // The pre-existing normal item stays above the Human section.
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
    let out = pwk::run_args(&args).unwrap();
    assert!(out.starts_with("ADDED PWF TASK [GLP-0002]"), "got: {out}");
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
    let err = pwk::run_args(&args).unwrap_err();
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
    let err = pwk::run_args(&args).unwrap_err();
    assert!(err.contains("no work-item prefix"), "got: {err}");
}

// ── Task 13 ──────────────────────────────────────────────────────────────────

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
    let out = pwk::run_args(&args).unwrap();
    assert!(
        out.contains("GLP-0001 :: tray gui"),
        "item line missing: {out}"
    );
}

#[test]
fn list_scopes_select_default_human_future_and_all() {
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
    let base = [
        "--config-path",
        &cfg_s,
        "--notes-dir",
        &notes_s,
        "--date",
        "2026-01-01",
    ];
    let run = |extra: &[&str]| {
        let mut argv = vec!["list"];
        argv.extend_from_slice(extra);
        argv.extend_from_slice(&base);
        pwk::run_args(&parse_args(&argv)).unwrap()
    };
    // Default: only normal tasks shown; scoped sections hidden.
    let def = run(&[]);
    assert!(def.contains("GLP-0001"), "normal missing: {def}");
    assert!(
        !def.contains("GLP-0002"),
        "Low-prio shown by default: {def}"
    );
    assert!(!def.contains("GLP-0003"), "Human shown by default: {def}");
    assert!(!def.contains("GLP-0004"), "Future shown by default: {def}");

    let h = run(&["--human"]);
    assert!(!h.contains("GLP-0001"), "normal leaked with --human: {h}");
    assert!(!h.contains("GLP-0002"), "low-prio leaked with --human: {h}");
    assert!(h.contains("GLP-0003"), "Human not shown with --human: {h}");
    assert!(!h.contains("GLP-0004"), "Future leaked with --human: {h}");

    let f = run(&["--future"]);
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

    let all = run(&["--all"]);
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
    let out = pwk::run_args(&parse_args(&[
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
    let out = pwk::run_args(&parse_args(&[
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

    // Grouped order: Default → Low-prio → Human → Future (cross-project).
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
    let out = pwk::run_args(&parse_args(&[
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
        pwk::run_args(&parse_args(&argv)).unwrap()
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
    let out = pwk::run_args(&args).unwrap();
    assert!(out.contains("GLP-0001 :: tray gui"), "got: {out}");
    assert!(out.contains("  status: READY"), "got: {out}");
    assert!(out.contains("  repo: /repo"), "got: {out}");
    assert!(out.contains("  prompt: add startup toggle"), "got: {out}");
}

// ── PWF-0020: list item cap (`-n`), per-project newest-first ordering ─────────

/// Stage `count` open glep-shimeji items (GLP-0001..=GLP-{count}) + config; return
/// the base argv tail (config/notes/date) used by the cap tests below.
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
        index.push_str(&format!("- [ ] [[{id}|t{n}]]\n"));
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
    pwk::run_args(&parse_args(&argv)).unwrap()
}

#[test]
fn list_groups_projects_before_sorting_by_number() {
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

    let out = list_run(&cfg, &notes, &["-n", "0"]);

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
    // Newest 10 kept (GLP-0012..=GLP-0003); the 2 lowest hidden.
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
    // Cap: exactly 10 items shown (2 hidden), newest first.
    assert!(out.contains("GLP-0012"), "newest missing: {out}");
    assert!(out.contains("GLP-0003"), "10th item missing: {out}");
    assert!(!out.contains("GLP-0002"), "11th item leaked: {out}");
    assert!(!out.contains("GLP-0001"), "12th item leaked: {out}");
    // Footer present when items are hidden.
    assert!(out.contains("2 more"), "hidden-count footer missing: {out}");
    assert!(out.contains("-n 0"), "escape hatch missing: {out}");
    // Newest-first: GLP-0012 appears before GLP-0003.
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
        proj.join("CFG-0020.md"),
        "---\nstatus: active\ntitle: dependent\nproject: config-handler\ncreated: 2026-01-01\nprereq: \"[[CFG-0014]]\"\n---\n\ndo the dependent thing\n",
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
    let out = pwk::run_args(&args).unwrap();
    assert!(out.contains("prereq: CFG-0014 (done)"), "got: {out}");
}

// ── Task 14 ──────────────────────────────────────────────────────────────────

#[test]
fn check_keeps_done_link_in_index_in_place_without_bak() {
    // PWF-0026: check now KEEPS the item as `- [x] [[ID]] ✅ date` in place
    // (a rotating done-queue), instead of deleting the index link.
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
        "check",
        "--id",
        "GLP-0001",
        "--config-path",
        &cfg.to_string_lossy(),
        "--notes-dir",
        &notes.to_string_lossy(),
        "--date",
        "2026-01-01",
    ]);
    let out = pwk::run_args(&args).unwrap();
    assert!(out.starts_with("Checked GLP-0001"), "got: {out}");
    // item file should now have status: done + completed
    let item = fs::read_to_string(proj.join("GLP-0001.md")).unwrap();
    assert!(item.contains("status: done"));
    assert!(item.contains("completed: 2026-01-01"));
    // index keeps the link, marked done in place, layout preserved.
    let index = fs::read_to_string(proj.join("glep-shimeji.md")).unwrap();
    assert_eq!(
        index,
        "# glep-shimeji\n\n- [x] [[GLP-0001]] ✅ 2026-01-01\n\n## Later\n"
    );
    // notes-pro is git-tracked: writes must NOT leave .bak clutter.
    assert!(!proj.join("GLP-0001.md.bak").exists());
    assert!(!proj.join("glep-shimeji.md.bak").exists());
}

#[test]
fn check_evicts_and_archives_oldest_beyond_general_cap() {
    // General cap is 6: with 6 done + a 7th checked, the oldest is unlinked and
    // its backing note moved to _archive/.
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
        index.push_str(&format!("- [x] [[{id}]] ✅ 2026-01-{n:02}\n"));
    }
    // The open 7th item we will check.
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
        "check",
        "--id",
        "GLP-0007",
        "--config-path",
        &cfg.to_string_lossy(),
        "--notes-dir",
        &notes.to_string_lossy(),
        "--date",
        "2026-06-13",
    ]);
    pwk::run_args(&args).unwrap();
    let index = fs::read_to_string(proj.join("glep-shimeji.md")).unwrap();
    // Oldest (GLP-0001) evicted from the index; GLP-0007 kept; six done remain.
    assert!(!index.contains("GLP-0001"), "oldest unlinked: {index}");
    assert!(index.contains("- [x] [[GLP-0007]] ✅ 2026-06-13"));
    assert_eq!(index.matches("- [x]").count(), 6);
    // Evicted note archived, not deleted.
    assert!(
        proj.join("_archive/GLP-0001.md").exists(),
        "evicted note archived"
    );
    assert!(
        !proj.join("GLP-0001.md").exists(),
        "moved out of project dir"
    );
}

// ── PWF-0015: update verb ─────────────────────────────────────────────────────

/// Shared staging for update tests: a project with one active GLP-0001 item.
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
    let out = pwk::run_args(&args).unwrap();
    assert!(
        out.contains("Updated GLP-0001") && out.contains("glep-shimeji :: tray gui"),
        "expected update confirmation for GLP-0001: {out}"
    );

    let item = fs::read_to_string(proj.join("GLP-0001.md")).unwrap();
    // Body is note_body-wrapped (Goals template, one bullet per slash lane).
    assert!(
        item.contains("## Goals\n- new prompt\n- second goal"),
        "body should be note_body-wrapped: {item}"
    );
    assert!(!item.contains("old prompt"), "old body replaced: {item}");
    // Frontmatter preserved untouched.
    assert!(item.contains("status: active"));
    assert!(item.contains("title: tray gui"));
    assert!(item.contains("project: glep-shimeji"));
    assert!(item.contains("created: 2026-01-01"));
    assert!(!item.contains("completed:"), "no completed added: {item}");
    // No .bak clutter.
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
    pwk::run_args(&args).unwrap();
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
    pwk::run_args(&args).unwrap();
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
    pwk::run_args(&args).unwrap();
    let item = fs::read_to_string(proj.join("GLP-0001.md")).unwrap();
    // Placeholder must NOT be Goals-wrapped, else is_placeholder_prompt can't flag it.
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
    let err = pwk::run_args(&args).unwrap_err();
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
    assert!(pwk::run_args(&args).is_err());
}

#[test]
fn check_with_report_appends_report_section() {
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
        "check",
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
    pwk::run_args(&args).unwrap();
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

    let err = pwk::run_args(&args).unwrap_err();

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

    let out = pwk::run_args(&args).unwrap();

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
        "# glep-shimeji\n\n- [x] [[GLP-0001]] ✅ 2026-01-01\n"
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
    let out = pwk::run_args(&args).unwrap();
    assert!(
        out.contains("REMOVED PWF TASK [PWF-0001]") && out.contains("pwf :: stale task"),
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

// ── Task 15 ──────────────────────────────────────────────────────────────────

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
    let out = pwk::run_args(&args).unwrap();
    assert!(out.starts_with("ADDED PWF TASK [GLP-0001]"), "got: {out}");
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
    pwk::run_args(&args).unwrap();
    let item = fs::read_to_string(proj.join("GLP-0002.md")).unwrap();
    assert!(
        item.contains("prereq: \"[[GLP-0001]]\"\n---"),
        "got: {item}"
    );
}

#[test]
fn route_create_verbs_error_with_add_hint() {
    // The old route create verbs (add/a/add-titled/at) no longer create; they
    // error pointing at the single canonical `pw add` form.
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
        let args = parse_args(&argv);
        let err = pwk::run_args(&args).unwrap_err();
        assert_eq!(err, expected);
    }
}

// ── resolve action ───────────────────────────────────────────────────────────

fn resolve_stage() -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
    let stage = stage_dir();
    let notes = stage.join("notes");
    let proj = notes.join("glep-shimeji");
    fs::create_dir_all(&proj).unwrap();
    fs::write(
        proj.join("GLP-0001.md"),
        "---\nstatus: active\ntitle: tray gui\nproject: glep-shimeji\ncreated: 2026-01-01\n---\n\nadd startup toggle\n",
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

fn resolve_args(cfg: &std::path::Path, notes: &std::path::Path, extra: &[&str]) -> pwf::cli::Args {
    let cfg_s = cfg.to_string_lossy();
    let notes_s = notes.to_string_lossy();
    let mut argv = vec![
        "resolve",
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
fn resolve_prints_item_note_path_plain() {
    let (notes, proj, cfg) = resolve_stage();
    let out = pwk::run_args(&resolve_args(&cfg, &notes, &[])).unwrap();
    assert_eq!(
        out.trim(),
        proj.join("GLP-0001.md").to_string_lossy().as_ref()
    );
}

#[test]
fn resolve_unknown_id_errors() {
    let (notes, _proj, cfg) = resolve_stage();
    let mut args = resolve_args(&cfg, &notes, &[]);
    args.id = Some("GLP-0099".to_string());
    let err = pwk::run_args(&args).unwrap_err();
    assert!(err.contains("GLP-0099"), "error should name the id: {err}");
}

#[test]
fn resolve_requires_id() {
    let (notes, _proj, cfg) = resolve_stage();
    let mut args = resolve_args(&cfg, &notes, &[]);
    args.id = None;
    let err = pwk::run_args(&args).unwrap_err();
    assert!(err.contains("--id"), "error should mention --id: {err}");
}

// `verify` rendering (claude + codex, item-present and item-absent) is covered
// white-box in `agent/verify.rs`, where the crate-internal `AgentLauncher` seam is
// reachable; no integration duplicate is kept here.

// ── clean action ──────────────────────────────────────────────────────────────

fn clean_stage() -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
    let stage = stage_dir();
    let notes = stage.join("notes");
    let proj = notes.join("config-handler");
    fs::create_dir_all(&proj).unwrap();
    let cfg = stage.join("config.json");
    fs::write(
        &cfg,
        format!(
            r#"{{ "notesDir": "{}", "projects": {{ "config-handler": "/repo" }}, "prefixes": {{ "config-handler": "CFG" }} }}"#,
            json_path(&notes)
        ),
    )
    .unwrap();
    (stage, proj, cfg)
}

fn active_item(title: &str) -> String {
    format!(
        "---\nstatus: active\ntitle: {title}\nproject: config-handler\ncreated: 2026-06-01\n---\n\nbody\n"
    )
}

#[test]
fn clean_sweeps_checked_links_sets_done_and_unlinks() {
    let (_stage, proj, cfg) = clean_stage();
    fs::write(proj.join("CFG-0012.md"), active_item("obsidian")).unwrap();
    fs::write(proj.join("CFG-0001.md"), active_item("bare open")).unwrap();
    fs::write(
        proj.join("config-handler.md"),
        "- [x] [[CFG-0012|obsidian]] ✅ 2026-06-05\n- [ ] [[CFG-0001|bare open]]\n",
    )
    .unwrap();
    let args = parse_args(&[
        "clean",
        "--force",
        "--config-path",
        &cfg.to_string_lossy(),
        "--notes-dir",
        &proj.parent().unwrap().to_string_lossy(),
        "--date",
        "2026-01-01",
    ]);
    let out = pwk::run_args(&args).unwrap();
    assert!(out.contains("CLEANED"), "got: {out}");
    assert!(out.contains("CFG-0012"), "got: {out}");
    let item = fs::read_to_string(proj.join("CFG-0012.md")).unwrap();
    assert!(item.contains("status: done"), "got: {item}");
    assert!(item.contains("completed: 2026-06-05"), "got: {item}");
    let idx = fs::read_to_string(proj.join("config-handler.md")).unwrap();
    assert!(!idx.contains("CFG-0012"), "link not removed: {idx}");
    assert!(
        idx.contains("- [ ] [[CFG-0001|bare open]]"),
        "open link lost: {idx}"
    );
    // notes-pro is git-tracked: the cleaner must NOT leave .bak clutter.
    assert!(!proj.join("CFG-0012.md.bak").exists());
    assert!(!proj.join("config-handler.md.bak").exists());
}

#[test]
fn clean_dry_run_writes_nothing() {
    let (_stage, proj, cfg) = clean_stage();
    fs::write(proj.join("CFG-0012.md"), active_item("obsidian")).unwrap();
    fs::write(
        proj.join("config-handler.md"),
        "- [x] [[CFG-0012|obsidian]] ✅ 2026-06-05\n",
    )
    .unwrap();
    let args = parse_args(&[
        "clean",
        "--dry-run",
        "--config-path",
        &cfg.to_string_lossy(),
        "--notes-dir",
        &proj.parent().unwrap().to_string_lossy(),
        "--date",
        "2026-01-01",
    ]);
    let out = pwk::run_args(&args).unwrap();
    assert!(out.contains("WOULD CLEAN"), "got: {out}");
    let item = fs::read_to_string(proj.join("CFG-0012.md")).unwrap();
    assert!(item.contains("status: active"), "file mutated: {item}");
    assert!(!item.contains("completed:"), "file mutated: {item}");
    let idx = fs::read_to_string(proj.join("config-handler.md")).unwrap();
    assert!(idx.contains("CFG-0012"), "link removed in dry run: {idx}");
    assert!(!proj.join("CFG-0012.md.bak").exists());
    assert!(!proj.join("config-handler.md.bak").exists());
}

#[test]
fn clean_skips_when_item_file_missing() {
    let (_stage, proj, cfg) = clean_stage();
    fs::write(proj.join("CFG-0012.md"), active_item("obsidian")).unwrap();
    // CFG-9999 has no backing file.
    fs::write(
        proj.join("config-handler.md"),
        "- [x] [[CFG-9999|ghost]] ✅ 2026-06-05\n- [x] [[CFG-0012|obsidian]] ✅ 2026-06-05\n",
    )
    .unwrap();
    let args = parse_args(&[
        "clean",
        "--force",
        "--config-path",
        &cfg.to_string_lossy(),
        "--notes-dir",
        &proj.parent().unwrap().to_string_lossy(),
        "--date",
        "2026-01-01",
    ]);
    let out = pwk::run_args(&args).unwrap();
    assert!(out.contains("CLEANED"), "got: {out}");
    assert!(
        out.contains("CFG-9999") && out.contains("skipped"),
        "got: {out}"
    );
    let idx = fs::read_to_string(proj.join("config-handler.md")).unwrap();
    // skipped link is retained; cleaned link is gone
    assert!(idx.contains("CFG-9999"), "skipped link removed: {idx}");
    assert!(!idx.contains("CFG-0012"), "cleaned link retained: {idx}");
}

#[test]
fn clean_falls_back_to_date_without_checkmark() {
    let (_stage, proj, cfg) = clean_stage();
    fs::write(proj.join("CFG-0012.md"), active_item("obsidian")).unwrap();
    fs::write(
        proj.join("config-handler.md"),
        "- [x] [[CFG-0012|obsidian]]\n",
    )
    .unwrap();
    let args = parse_args(&[
        "clean",
        "--force",
        "--config-path",
        &cfg.to_string_lossy(),
        "--notes-dir",
        &proj.parent().unwrap().to_string_lossy(),
        "--date",
        "2026-03-03",
    ]);
    let out = pwk::run_args(&args).unwrap();
    assert!(out.contains("CLEANED"), "got: {out}");
    let item = fs::read_to_string(proj.join("CFG-0012.md")).unwrap();
    assert!(item.contains("completed: 2026-03-03"), "got: {item}");
}

#[test]
fn clean_project_filter_limits_sweep() {
    let stage = stage_dir();
    let notes = stage.join("notes");
    let a = notes.join("config-handler");
    let b = notes.join("glep-shimeji");
    fs::create_dir_all(&a).unwrap();
    fs::create_dir_all(&b).unwrap();
    fs::write(a.join("CFG-0012.md"), active_item("obsidian")).unwrap();
    fs::write(
        b.join("GLP-0001.md"),
        "---\nstatus: active\ntitle: tray\nproject: glep-shimeji\ncreated: 2026-06-01\n---\n\nbody\n",
    )
    .unwrap();
    fs::write(
        a.join("config-handler.md"),
        "- [x] [[CFG-0012|obsidian]] ✅ 2026-06-05\n",
    )
    .unwrap();
    fs::write(
        b.join("glep-shimeji.md"),
        "- [x] [[GLP-0001|tray]] ✅ 2026-06-05\n",
    )
    .unwrap();
    let cfg = stage.join("config.json");
    fs::write(
        &cfg,
        format!(
            r#"{{ "notesDir": "{}", "projects": {{ "config-handler": "/r1", "glep-shimeji": "/r2" }}, "prefixes": {{ "config-handler": "CFG", "glep-shimeji": "GLP" }} }}"#,
            json_path(&notes)
        ),
    )
    .unwrap();
    let args = parse_args(&[
        "clean",
        "--project",
        "config-handler",
        "--force",
        "--config-path",
        &cfg.to_string_lossy(),
        "--notes-dir",
        &notes.to_string_lossy(),
        "--date",
        "2026-01-01",
    ]);
    let out = pwk::run_args(&args).unwrap();
    assert!(out.contains("CLEANED"), "got: {out}");
    assert!(out.contains("CFG-0012"), "got: {out}");
    // glep-shimeji untouched
    let gidx = fs::read_to_string(b.join("glep-shimeji.md")).unwrap();
    assert!(gidx.contains("GLP-0001"), "other project swept: {gidx}");
}

#[test]
fn route_clean_verb_sweeps_project() {
    // Route doesn't expose --force; use --dry-run to confirm the clean verb is
    // recognized and dispatched without hitting the apply gate.
    let (_stage, proj, cfg) = clean_stage();
    fs::write(proj.join("CFG-0012.md"), active_item("obsidian")).unwrap();
    fs::write(
        proj.join("config-handler.md"),
        "- [x] [[CFG-0012|obsidian]] ✅ 2026-06-05\n",
    )
    .unwrap();
    let args = parse_args(&[
        "clean",
        "--dry-run",
        "--project",
        "config-handler",
        "--config-path",
        &cfg.to_string_lossy(),
        "--notes-dir",
        &proj.parent().unwrap().to_string_lossy(),
        "--date",
        "2026-01-01",
    ]);
    let out = pwk::run_args(&args).unwrap();
    assert!(out.contains("WOULD CLEAN"), "got: {out}");
    assert!(out.contains("CFG-0012"), "got: {out}");
    let item = fs::read_to_string(proj.join("CFG-0012.md")).unwrap();
    assert!(
        item.contains("status: active"),
        "mutated in dry-run: {item}"
    );
}

#[test]
fn clean_reports_nothing_when_no_checked_links() {
    let (_stage, proj, cfg) = clean_stage();
    fs::write(proj.join("CFG-0012.md"), active_item("obsidian")).unwrap();
    // Only an open link — nothing to clean.
    fs::write(
        proj.join("config-handler.md"),
        "- [ ] [[CFG-0012|obsidian]]\n",
    )
    .unwrap();
    let base = [
        "--config-path",
        &*cfg.to_string_lossy(),
        "--notes-dir",
        &*proj.parent().unwrap().to_string_lossy(),
        "--date",
        "2026-01-01",
    ];
    let mut text_argv = vec!["clean"];
    text_argv.extend_from_slice(&base);
    let out = pwk::run_args(&parse_args(&text_argv)).unwrap();
    assert!(out.contains("No done work-item links"), "got: {out}");
    // Nothing mutated, no backup written.
    let idx = fs::read_to_string(proj.join("config-handler.md")).unwrap();
    assert!(idx.contains("- [ ] [[CFG-0012|obsidian]]"));
    assert!(!proj.join("config-handler.md.bak").exists());
}

fn load_clean_cfg(cfg: &std::path::Path) -> pwf::config::Config {
    pwf::config::load(&cfg.to_string_lossy(), None).unwrap()
}

fn stage_one_done() -> (std::path::PathBuf, std::path::PathBuf) {
    let (_stage, proj, cfg) = clean_stage();
    fs::write(proj.join("CFG-0012.md"), active_item("obsidian")).unwrap();
    fs::write(
        proj.join("config-handler.md"),
        "- [x] [[CFG-0012|obsidian]] ✅ 2026-06-05\n",
    )
    .unwrap();
    (proj, cfg)
}

#[test]
fn clean_confirm_yes_applies_without_bak() {
    let (proj, cfg) = stage_one_done();
    let confirmer = FakeConfirm {
        interactive: true,
        answer: true,
    };
    let out = clean::run_clean(
        &load_clean_cfg(&cfg),
        None,
        "2026-01-01",
        false,
        false,
        &confirmer,
    )
    .unwrap();
    assert!(out.contains("CLEANED"), "got: {out}");
    let item = fs::read_to_string(proj.join("CFG-0012.md")).unwrap();
    assert!(item.contains("status: done"));
    assert!(item.contains("completed: 2026-06-05"));
    let idx = fs::read_to_string(proj.join("config-handler.md")).unwrap();
    assert!(!idx.contains("CFG-0012"));
    // The whole point: no .bak clutter (notes-pro is git-tracked).
    assert!(!proj.join("CFG-0012.md.bak").exists());
    assert!(!proj.join("config-handler.md.bak").exists());
}

#[test]
fn clean_confirm_no_aborts_and_keeps_everything() {
    let (proj, cfg) = stage_one_done();
    let confirmer = FakeConfirm {
        interactive: true,
        answer: false,
    };
    let out = clean::run_clean(
        &load_clean_cfg(&cfg),
        None,
        "2026-01-01",
        false,
        false,
        &confirmer,
    )
    .unwrap();
    assert!(out.contains("Aborted"), "got: {out}");
    let item = fs::read_to_string(proj.join("CFG-0012.md")).unwrap();
    assert!(item.contains("status: active"), "must not mutate: {item}");
    let idx = fs::read_to_string(proj.join("config-handler.md")).unwrap();
    assert!(
        idx.contains("- [x] [[CFG-0012|obsidian]]"),
        "link must remain: {idx}"
    );
}

#[test]
fn clean_non_interactive_without_force_refuses() {
    let (proj, cfg) = stage_one_done();
    let confirmer = FakeConfirm {
        interactive: false,
        answer: true,
    };
    let err = clean::run_clean(
        &load_clean_cfg(&cfg),
        None,
        "2026-01-01",
        false,
        false,
        &confirmer,
    )
    .unwrap_err();
    assert!(
        err.contains("--force") || err.contains("--dry-run"),
        "got: {err}"
    );
    let item = fs::read_to_string(proj.join("CFG-0012.md")).unwrap();
    assert!(item.contains("status: active"), "must not mutate: {item}");
}

#[test]
fn clean_force_applies_without_prompt() {
    let (proj, cfg) = stage_one_done();
    // answer:false would refuse, but --force bypasses the gate entirely.
    let confirmer = FakeConfirm {
        interactive: false,
        answer: false,
    };
    let out = clean::run_clean(
        &load_clean_cfg(&cfg),
        None,
        "2026-01-01",
        false,
        true,
        &confirmer,
    )
    .unwrap();
    assert!(out.contains("CLEANED"), "got: {out}");
    let item = fs::read_to_string(proj.join("CFG-0012.md")).unwrap();
    assert!(item.contains("status: done"));
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn stage_dir() -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("pwstage_{}", nanos()));
    fs::create_dir_all(&d).unwrap();
    d
}

fn parse_args(argv: &[&str]) -> pwf::cli::Args {
    let mut v = vec!["pw".to_string()];
    v.extend(argv.iter().map(|s| s.to_string()));
    pwf::command::parse_argv(v).unwrap().1
}

/// JSON-escape a path (forward slashes for JSON string, escaped backslashes on Windows).
fn json_path(p: &std::path::Path) -> String {
    p.to_string_lossy().replace('\\', "\\\\")
}

// ── Existing tests below ─────────────────────────────────────────────────────

#[test]
fn next_id_is_max_plus_one_first_is_0001() {
    let dir = std::env::temp_dir().join(format!("pwid_{}", nanos()));
    fs::create_dir_all(&dir).unwrap();
    assert_eq!(pwk::next_work_item_id(&dir, "GLP"), "GLP-0001");
    fs::write(dir.join("GLP-0001.md"), "x").unwrap();
    fs::write(dir.join("GLP-0003.md"), "x").unwrap(); // gap preserved
    fs::write(dir.join("OTHER.md"), "x").unwrap(); // ignored
    assert_eq!(pwk::next_work_item_id(&dir, "GLP"), "GLP-0004");
}

fn nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}

#[test]
fn parses_file_model_items_with_line_numbers() {
    let dir = std::env::temp_dir().join(format!("pwtasks_{}", nanos()));
    let proj = dir.join("glep-shimeji");
    std::fs::create_dir_all(&proj).unwrap();
    std::fs::write(proj.join("GLP-0001.md"),
        "---\nstatus: active\ntitle: tray gui\nproject: glep-shimeji\ncreated: 2026-01-01\n---\n\nadd startup toggle\n").unwrap();
    std::fs::write(proj.join("GLP-0002.md"),
        "---\nstatus: active\ntitle: status bar\nproject: glep-shimeji\ncreated: 2026-01-01\n---\n\nadd current mood indicator\n").unwrap();
    let index = proj.join("glep-shimeji.md");
    std::fs::write(
        &index,
        "# glep-shimeji\n- [[GLP-0001|tray gui]]\n\n- [[GLP-0002|status bar]]\n",
    )
    .unwrap();

    let items = pwk::get_project_tasks("glep-shimeji", Some("/repo"), &index);
    assert_eq!(items[0].id, "GLP-0001");
    assert_eq!(items[0].session, "tray gui");
    assert_eq!(items[0].prompt, "add startup toggle");
    assert_eq!(items[0].line, 2);
    assert_eq!(items[0].format, "file");
    assert!(items[0].launchable);
    // PS regex ^\s*-\s*\[\[ has \s* consuming the preceding blank line,
    // so marker_index lands on the blank-line position (line 3), not line 4.
    // Get-LineNumber counts newlines before $m.Index: 2 → returns 3.
    assert_eq!(items[1].line, 3);
}

#[test]
fn work_item_content_active_then_done_field_order() {
    let active = pwk::work_item_content(
        "tray gui",
        "glep-shimeji",
        "add startup toggle",
        "active",
        "2026-01-01",
        None,
        None,
        None,
    );
    assert!(active.starts_with(
        "---\nstatus: active\ntitle: tray gui\nproject: glep-shimeji\ncreated: 2026-01-01\n---\n"
    ));
    assert!(active.contains("\nadd startup toggle\n"));
    // Set-WorkItemStatus inserts `completed:` right after `status:`
    let done = pwk::set_status_text(&active, "done", "2026-02-02");
    assert!(done.contains("status: done\ncompleted: 2026-02-02\n"));
}

#[test]
fn add_then_remove_index_link_top_placement() {
    let c = pwk::add_link_to_index("# glep-shimeji\n\n## Later\n", "- [[GLP-0001|tray gui]]");
    assert!(c.contains("- [[GLP-0001|tray gui]]"));
    let removed = pwk::remove_index_link(&c, "GLP-0001");
    assert!(!removed.contains("GLP-0001"));
}
