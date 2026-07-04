use std::{assert_matches, error::Error, fs};

use pwf::{cli, engines::migrate};

fn nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}

fn stage_dir() -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("migrate_test_{}", nanos()));
    fs::create_dir_all(&d).unwrap();
    d
}

fn parse_args(v: &[&str]) -> cli::Args {
    let mut owned = vec!["migrate".to_string()];
    owned.extend(v.iter().map(std::string::ToString::to_string));
    pwf::command::parse_argv(owned).unwrap().1
}

#[test]
fn migrate_converts_all() {
    let stage = stage_dir();
    let notes_dir = stage.join("notes");
    fs::create_dir_all(&notes_dir).unwrap();

    // Write the flat note (matches fixture input)
    let flat_note = notes_dir.join("glep-shimeji.md");
    fs::write(
        &flat_note,
        "# glep-shimeji\n\nHuman intro stays.\n\n- [x] `old cleanup` :: completed cleanup on 2025-12-31\n- [ ] `tray gui` <- add startup toggle\n\n## Human Notes\n\nKeep this section.\n",
    )
    .unwrap();

    // Write config
    let cfg_path = stage.join("config.json");
    fs::write(
        &cfg_path,
        format!(
            r#"{{"notesDir": {notes_json}, "projects": {{"glep-shimeji": "/repo"}}, "prefixes": {{"glep-shimeji": "GLP"}}}}"#,
            notes_json = serde_json::to_string(notes_dir.to_str().unwrap()).unwrap()
        ),
    )
    .unwrap();

    let args = parse_args(&[
        "--config-path",
        cfg_path.to_str().unwrap(),
        "--notes-dir",
        notes_dir.to_str().unwrap(),
        "--date",
        "2026-01-01",
    ]);

    migrate::run(&args).unwrap();

    // GLP-0001 = tray gui (last document item, active)
    let glp1 = fs::read_to_string(notes_dir.join("glep-shimeji/GLP-0001.md")).unwrap();
    assert!(glp1.contains("status: active"), "GLP-0001 should be active");
    assert!(glp1.contains("title: tray gui"), "GLP-0001 title");
    assert!(
        glp1.contains("created: 2026-01-01"),
        "GLP-0001 created date"
    );
    assert!(glp1.contains("add startup toggle"), "GLP-0001 body");
    assert!(
        !glp1.contains("completed:"),
        "GLP-0001 should have no completed field"
    );

    // GLP-0002 = old cleanup (first document item, done)
    let glp2 = fs::read_to_string(notes_dir.join("glep-shimeji/GLP-0002.md")).unwrap();
    assert!(glp2.contains("status: done"), "GLP-0002 should be done");
    assert!(glp2.contains("title: old cleanup"), "GLP-0002 title");
    assert!(
        glp2.contains("created: 2025-12-31"),
        "GLP-0002 created = completed date"
    );
    assert!(glp2.contains("completed: 2025-12-31"), "GLP-0002 completed");
    assert!(
        glp2.contains("completed cleanup on 2025-12-31"),
        "GLP-0002 body"
    );
    // completed must be the LAST frontmatter field (after created)
    let created_pos = glp2.find("created:").unwrap();
    let completed_pos = glp2.find("completed:").unwrap();
    assert!(
        completed_pos > created_pos,
        "completed must come after created in frontmatter"
    );

    // Index: active links prepended (with "- [ ]" Obsidian checkbox), legacy inline agent items
    // converted
    let index = fs::read_to_string(notes_dir.join("glep-shimeji/glep-shimeji.md")).unwrap();
    assert!(
        index.starts_with("- [ ] [[GLP-0001]]"),
        "index starts with active checkbox link"
    );
    // Done items are not re-added as active links, and no legacy inline backtick tasks remain.
    assert!(
        !index.contains("- [x]") && !index.contains('`'),
        "legacy inline agent items converted"
    );
    assert!(
        index.contains("Human intro stays."),
        "human content retained"
    );
    assert!(index.contains("## Human Notes"), "human sections retained");
    assert!(
        index.contains("Keep this section."),
        "human content retained"
    );

    // Flat note deleted
    assert!(
        !flat_note.exists(),
        "flat note should be deleted after migration"
    );
}

#[test]
fn migrate_dry_run_does_not_write() {
    let stage = stage_dir();
    let notes_dir = stage.join("notes");
    fs::create_dir_all(&notes_dir).unwrap();

    let flat_note = notes_dir.join("glep-shimeji.md");
    fs::write(
        &flat_note,
        "# glep-shimeji\n\n- [ ] `tray gui` <- add startup toggle\n",
    )
    .unwrap();

    let cfg_path = stage.join("config.json");
    fs::write(
        &cfg_path,
        format!(
            r#"{{"notesDir": {notes_json}, "projects": {{}}, "prefixes": {{"glep-shimeji": "GLP"}}}}"#,
            notes_json = serde_json::to_string(notes_dir.to_str().unwrap()).unwrap()
        ),
    )
    .unwrap();

    let args = parse_args(&[
        "--config-path",
        cfg_path.to_str().unwrap(),
        "--notes-dir",
        notes_dir.to_str().unwrap(),
        "--date",
        "2026-01-01",
        "--dry-run",
    ]);

    migrate::run(&args).unwrap();

    // Flat note must still exist (dry run)
    assert!(flat_note.exists(), "flat note must survive dry run");
    // Folder must not be created
    assert!(
        !notes_dir.join("glep-shimeji").exists(),
        "folder must not be created in dry run"
    );
}

#[test]
fn migrate_skips_when_folder_index_exists() {
    let stage = stage_dir();
    let notes_dir = stage.join("notes");
    let folder = notes_dir.join("glep-shimeji");
    fs::create_dir_all(&folder).unwrap();

    // Pre-existing folder index
    fs::write(folder.join("glep-shimeji.md"), "already migrated\n").unwrap();

    // Flat note also exists
    let flat_note = notes_dir.join("glep-shimeji.md");
    fs::write(&flat_note, "# glep-shimeji\n\n- [ ] `item` <- do thing\n").unwrap();

    let cfg_path = stage.join("config.json");
    fs::write(
        &cfg_path,
        format!(
            r#"{{"notesDir": {notes_json}, "projects": {{}}, "prefixes": {{"glep-shimeji": "GLP"}}}}"#,
            notes_json = serde_json::to_string(notes_dir.to_str().unwrap()).unwrap()
        ),
    )
    .unwrap();

    let args = parse_args(&[
        "--config-path",
        cfg_path.to_str().unwrap(),
        "--notes-dir",
        notes_dir.to_str().unwrap(),
        "--date",
        "2026-01-01",
    ]);

    migrate::run(&args).unwrap();

    // Flat note must be untouched (skipped)
    assert!(
        flat_note.exists(),
        "flat note untouched when folder index exists"
    );
    // Existing folder index unchanged
    assert_eq!(
        fs::read_to_string(folder.join("glep-shimeji.md")).unwrap(),
        "already migrated\n"
    );
}

#[test]
fn run_typed_wraps_config_errors_with_legacy_display() {
    let stage = stage_dir();
    let cfg_path = stage.join("missing-config.json");
    let args = parse_args(&["--config-path", cfg_path.to_str().unwrap()]);

    let err = migrate::run_typed(&args).unwrap_err();

    assert_matches!(err, migrate::MigrateError::Config(_));
    assert_eq!(
        err.to_string(),
        format!("Pending work config not found: {}", cfg_path.display())
    );
    assert_eq!(
        err.source().map(ToString::to_string),
        Some(format!(
            "Pending work config not found: {}",
            cfg_path.display()
        ))
    );
    assert_eq!(migrate::run(&args).unwrap_err(), err.to_string());
}

#[test]
fn run_typed_returns_read_variant_with_legacy_display_and_source() {
    let stage = stage_dir();
    let notes_dir = stage.join("notes");
    fs::create_dir_all(&notes_dir).unwrap();

    let flat_note = notes_dir.join("glep-shimeji.md");
    fs::create_dir(&flat_note).unwrap();

    let cfg_path = stage.join("config.json");
    fs::write(
        &cfg_path,
        format!(
            r#"{{"notesDir": {notes_json}, "projects": {{}}, "prefixes": {{"glep-shimeji": "GLP"}}}}"#,
            notes_json = serde_json::to_string(notes_dir.to_str().unwrap()).unwrap()
        ),
    )
    .unwrap();

    let args = parse_args(&[
        "--config-path",
        cfg_path.to_str().unwrap(),
        "--notes-dir",
        notes_dir.to_str().unwrap(),
    ]);

    let err = migrate::run_typed(&args).unwrap_err();

    assert_matches!(
        &err,
        migrate::MigrateError::Read { path, .. } if path == &flat_note
    );
    let io_source_text = match &err {
        migrate::MigrateError::Read { source, .. } => source.to_string(),
        other => panic!("expected read error, got {other:?}"),
    };
    assert_eq!(
        err.to_string(),
        format!("Failed to read {}: {io_source_text}", flat_note.display())
    );
    assert_eq!(err.source().map(ToString::to_string), Some(io_source_text));
    assert_eq!(migrate::run(&args).unwrap_err(), err.to_string());
}

#[test]
fn migrate_io_errors_preserve_operation_display_text() {
    let source = || std::io::Error::other("disk said no");

    let err = migrate::MigrateError::Create {
        path: "/tmp/pwf-migrate/project".into(),
        source: source(),
    };
    assert_eq!(
        err.to_string(),
        "Failed to create /tmp/pwf-migrate/project: disk said no"
    );

    let err = migrate::MigrateError::Write {
        path: "/tmp/pwf-migrate/item.md".into(),
        source: source(),
    };
    assert_eq!(
        err.to_string(),
        "Failed to write /tmp/pwf-migrate/item.md: disk said no"
    );

    let err = migrate::MigrateError::Delete {
        path: "/tmp/pwf-migrate/flat.md".into(),
        source: source(),
    };
    assert_eq!(
        err.to_string(),
        "Failed to delete /tmp/pwf-migrate/flat.md: disk said no"
    );
}
