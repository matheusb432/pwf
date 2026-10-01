use crate::support::{assert_failure, command};

#[test]
fn redirected_tui_rejects_before_connecting_or_writing_terminal_escapes() {
    let root = tempfile::tempdir().unwrap();
    let output = command()
        .arg("tui")
        .env("PWF_RUNTIME_DIR", root.path())
        .output()
        .unwrap();
    assert_failure(output, &["interactive terminal", "pwf task list"]).unwrap();
}

#[cfg(target_os = "linux")]
mod terminal {
    use std::{
        fs,
        os::unix::fs::PermissionsExt as _,
        time::{Duration, Instant},
    };

    use expectrl::{Expect as _, session::OsSession};

    use crate::support::{DatabaseFixture, assert_success, task_id, task_json};

    struct Fixture {
        database: DatabaseFixture,
        directory: tempfile::TempDir,
    }

    impl Fixture {
        fn new() -> Self {
            let directory = tempfile::tempdir().unwrap();
            let database = DatabaseFixture::new(directory.path().join("tui.sqlite3")).unwrap();
            let source = directory.path().join("source");
            let tasks = directory.path().join("tasks");
            fs::create_dir_all(&source).unwrap();
            fs::create_dir_all(&tasks).unwrap();
            database.add_directory_project(
                &"foo".parse().unwrap(),
                "tui-contract",
                &source,
                &tasks,
            );
            let output = database
                .command()
                .args(["task", "add", "FOO", "Existing task / Saved search marker"])
                .output()
                .unwrap();
            assert_success(&output, "create terminal fixture task");
            Self {
                database,
                directory,
            }
        }

        fn spawn(&self, editor: Option<&std::path::Path>) -> Terminal {
            let mut command = self.database.command();
            command.args(["tui", "FOO"]).env("TERM", "xterm-256color");
            if let Some(editor) = editor {
                command.env("VISUAL", format!("'{}'", editor.display()));
            }
            let mut session = expectrl::Session::spawn(command).unwrap();
            session.set_expect_timeout(Some(Duration::from_secs(10)));
            session.get_process_mut().set_window_size(140, 40).unwrap();
            let mut terminal = Terminal {
                session,
                parser: vt100::Parser::new(40, 140, 0),
            };
            terminal.expect("Loaded 1 saved records");
            terminal
        }
    }

    struct Terminal {
        session: OsSession,
        parser: vt100::Parser,
    }

    impl Terminal {
        fn send(&mut self, input: &str) {
            self.session.send(input).unwrap();
        }

        fn expect(&mut self, text: &str) {
            let deadline = Instant::now() + Duration::from_secs(10);
            self.read_available().unwrap();
            while !self.parser.screen().contents().contains(text) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
                self.read_available().unwrap();
            }
            let screen = self.parser.screen().contents();
            assert!(
                screen.contains(text),
                "Terminal did not show {text:?}:\n{screen}"
            );
        }

        fn read_available(&mut self) -> anyhow::Result<()> {
            let mut bytes = [0; 8192];
            for _ in 0..256 {
                match self.session.try_read(&mut bytes) {
                    Ok(0) => anyhow::bail!("Terminal closed: {}", self.parser.screen().contents()),
                    Ok(count) => self.parser.process(&bytes[..count]),
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return Ok(()),
                    Err(error) => return Err(error.into()),
                }
            }
            Ok(())
        }
    }

    fn quit(terminal: &mut Terminal) {
        terminal.send("q");
        let output = terminal.session.expect(expectrl::Eof).unwrap();
        terminal.parser.process(output.as_bytes());
        assert!(
            !terminal.parser.screen().alternate_screen(),
            "quit must restore the normal screen"
        );
        assert!(matches!(
            terminal.session.get_process().wait().unwrap(),
            expectrl::process::unix::WaitStatus::Exited(_, 0)
        ));
    }

    #[test]
    fn creates_template_task_and_note_keeps_failed_editor_input_and_completes_with_report() {
        let fixture = Fixture::new();
        fixture.database.write_user_config("[task_body]\npreset = 'contract'\n[task_body.presets.contract]\nsections = [{marker='/g', title='Terminal goals', level=3}]\n").unwrap();
        let editor = fixture.directory.path().join("test editor");
        fs::write(&editor, "#!/bin/sh\nprintf 'editor-ready\\n'\nIFS= read -r input\nprintf '\\n%s\\n' \"$input\" >> \"$1\"\nexit 7\n").unwrap();
        fs::set_permissions(&editor, fs::Permissions::from_mode(0o755)).unwrap();
        let mut session = fixture.spawn(Some(&editor));
        session.send("t");
        session.expect("New task");
        session.send("Created through TUI\t\x05");
        session.expect("editor-ready");
        session.send("Retained after failed editor\n");
        session.expect("Edited input retained");
        session.send("\x13");
        session.expect("Created FOO-0002");
        let task = task_json(&fixture.database, &task_id("FOO-0002").unwrap()).unwrap();
        let body = task["body"].as_str().unwrap();
        assert!(body.contains("### Terminal goals"));
        assert!(body.contains("Retained after failed editor"));
        session.send("/Created through TUI\rD");
        session.expect("Complete FOO-0002");
        session.send("Verified terminal workflow\tbase..tip\x13");
        session.expect("Completed FOO-0002");
        let task = task_json(&fixture.database, &task_id("FOO-0002").unwrap()).unwrap();
        assert_eq!(task["status"], "done");
        assert!(
            task["body"]
                .as_str()
                .unwrap()
                .contains("Verified terminal workflow")
        );
        assert_eq!(task["commits"], "base..tip");
        session.send("n");
        session.expect("New note");
        session.send("Terminal note\tSaved note body\x13");
        session.expect("Created note FOO-NOTE-0001");
        let note =
            fs::read_to_string(fixture.directory.path().join("tasks/FOO-NOTE-0001.md")).unwrap();
        assert!(note.contains("# Terminal note"));
        assert!(note.contains("Saved note body"));
        quit(&mut session);
    }

    #[test]
    fn metadata_conflict_retains_edits_and_refresh_preserves_concurrent_title() {
        let fixture = Fixture::new();
        let mut session = fixture.spawn(None);
        session.send("m");
        session.expect("Edit FOO-0001");
        let output = fixture
            .database
            .command()
            .args([
                "task",
                "edit",
                "FOO-0001",
                "--title",
                "Changed outside the TUI",
            ])
            .output()
            .unwrap();
        assert_success(&output, "concurrent title edit");
        session.send("\tretained_tag\x13");
        session.expect("Draft retained");
        let task = task_json(&fixture.database, &task_id("FOO-0001").unwrap()).unwrap();
        assert_eq!(task["title"], "Changed outside the TUI");
        assert!(task["tags"].is_null());
        session.send("\x1b[15~");
        session.expect("Saved FOO-0001 is active");
        session.expect("Changed outside the TUI");
        session.send("\r");
        session.expect("Saved state inspected");
        session.send("\x13");
        session.expect("Updated FOO-0001");
        let task = task_json(&fixture.database, &task_id("FOO-0001").unwrap()).unwrap();
        assert_eq!(task["title"], "Changed outside the TUI");
        assert_eq!(task["tags"], serde_json::json!(["retained_tag"]));
        quit(&mut session);
    }

    #[test]
    fn exports_marked_references_after_the_server_stops() {
        let fixture = Fixture::new();
        let path = fixture.directory.path().join("references.md");
        let existing = fixture.directory.path().join("existing.md");
        fs::write(&existing, "keep existing text").unwrap();
        let mut terminal = fixture.spawn(None);
        drop(fixture.database);
        terminal.send(" x");
        terminal.expect("Export task references");
        terminal.send(&format!("{}\x13", existing.display()));
        terminal.expect("Draft retained. Choose a new path");
        terminal.send("\x01\x0b");
        terminal.send(&format!("{}\x13", path.display()));
        terminal.expect("Exported references");
        assert_eq!(fs::read_to_string(path).unwrap(), "[[FOO-0001]]\n");
        assert_eq!(fs::read_to_string(existing).unwrap(), "keep existing text");
        quit(&mut terminal);
    }

    #[test]
    fn delete_declines_by_default_and_reopen_keeps_the_server_confirmation() {
        let fixture = Fixture::new();
        let before = task_json(&fixture.database, &task_id("FOO-0001").unwrap()).unwrap();
        let mut session = fixture.spawn(None);
        session.send("\x1b[3~");
        session.expect("Delete FOO-0001?");
        session.expect("hard delete");
        session.send("\r");
        session.expect("Cancelled; no approval");
        assert_eq!(
            task_json(&fixture.database, &task_id("FOO-0001").unwrap()).unwrap(),
            before
        );
        session.send("d");
        session.expect("Completed FOO-0001");
        session.send("s");
        session.expect("backlog");
        session.send("s");
        session.expect("done");
        session.send("a");
        session.expect("Reopen FOO-0001?");
        session.expect("removes completion metadata");
        session.send("y\r");
        session.expect("Activated FOO-0001");
        assert_eq!(
            task_json(&fixture.database, &task_id("FOO-0001").unwrap()).unwrap()["status"],
            "active"
        );
        quit(&mut session);
    }
}
