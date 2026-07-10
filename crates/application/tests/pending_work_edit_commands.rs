use std::{
    error::Error,
    fmt,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use cqrsy::send_now;
use pwf_application::{
    AddItemSpec, AddPendingWorkError, AddPendingWorkItem, AddPendingWorkItemHandler,
    CancelItemSpec, CancelPendingWork, CancelPendingWorkError, CancelPendingWorkHandler,
    ClosedItem, ClosedItemAction, CompleteItemSpec, CompletePendingWork, CompletePendingWorkError,
    CompletePendingWorkHandler, PendingWorkWriteStore, RemovePendingWorkError,
    RemovePendingWorkItem, RemovePendingWorkItemHandler, ReopenPendingWork, ReopenPendingWorkError,
    ReopenPendingWorkHandler, ReopenedItem, StatusTransitionDiagnostics, UpdateItemSpec,
    UpdatePendingWorkError, UpdatePendingWorkItem, UpdatePendingWorkItemHandler,
};
use pwf_domain::pending_work::{AddedItem, RemovedItem, Tags, UpdatedItem};

#[derive(Clone, Default)]
struct RecordingWriteStore {
    state: Arc<Mutex<RecordingState>>,
}

#[derive(Default)]
struct RecordingState {
    add_specs: Vec<AddItemSpec>,
    update_specs: Vec<UpdateItemSpec>,
    remove_ids: Vec<String>,
    complete_specs: Vec<CompleteItemSpec>,
    cancel_specs: Vec<CancelItemSpec>,
    reopen_ids: Vec<String>,
    fail: bool,
}

#[derive(Debug, Clone)]
struct FakeStoreError;

impl fmt::Display for FakeStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("fake store failed")
    }
}

impl Error for FakeStoreError {}

impl RecordingWriteStore {
    fn failing() -> Self {
        let store = Self::default();
        store.state.lock().unwrap().fail = true;
        store
    }

    fn add_specs(&self) -> Vec<AddItemSpec> {
        self.state.lock().unwrap().add_specs.clone()
    }

    fn update_specs(&self) -> Vec<UpdateItemSpec> {
        self.state.lock().unwrap().update_specs.clone()
    }

    fn remove_ids(&self) -> Vec<String> {
        self.state.lock().unwrap().remove_ids.clone()
    }

    fn complete_specs(&self) -> Vec<CompleteItemSpec> {
        self.state.lock().unwrap().complete_specs.clone()
    }

    fn cancel_specs(&self) -> Vec<CancelItemSpec> {
        self.state.lock().unwrap().cancel_specs.clone()
    }

    fn reopen_ids(&self) -> Vec<String> {
        self.state.lock().unwrap().reopen_ids.clone()
    }
}

impl PendingWorkWriteStore for RecordingWriteStore {
    type Error = FakeStoreError;

    fn add_item(&self, spec: AddItemSpec) -> Result<AddedItem, Self::Error> {
        let mut state = self.state.lock().unwrap();
        if state.fail {
            return Err(FakeStoreError);
        }
        state.add_specs.push(spec);
        Ok(AddedItem {
            id: "PWF-0007".to_string(),
            project: "pwf".to_string(),
            title: "ship it".to_string(),
            note_path: PathBuf::from("/notes/pwf/PWF-0007.md"),
            created_section: None,
        })
    }

    fn update_item(&self, spec: UpdateItemSpec) -> Result<UpdatedItem, Self::Error> {
        let mut state = self.state.lock().unwrap();
        if state.fail {
            return Err(FakeStoreError);
        }
        state.update_specs.push(spec);
        Ok(UpdatedItem::OpenItemEdit {
            id: "PWF-0007".to_string(),
            project: "pwf".to_string(),
            title: "renamed".to_string(),
        })
    }

    fn remove_item(&self, id: &str) -> Result<RemovedItem, Self::Error> {
        let mut state = self.state.lock().unwrap();
        if state.fail {
            return Err(FakeStoreError);
        }
        state.remove_ids.push(id.to_string());
        Ok(RemovedItem {
            id: id.to_string(),
            project: "pwf".to_string(),
            title: "stale task".to_string(),
            deleted_path: PathBuf::from("/notes/pwf/PWF-0007.md"),
            unlinked: "/notes/pwf/pwf.md".to_string(),
        })
    }

    fn complete_item(&self, spec: CompleteItemSpec) -> Result<ClosedItem, Self::Error> {
        let mut state = self.state.lock().unwrap();
        if state.fail {
            return Err(FakeStoreError);
        }
        state.complete_specs.push(spec.clone());
        Ok(ClosedItem {
            id: "PWF-0007".to_string(),
            project: "pwf".to_string(),
            title: "ship it".to_string(),
            action: ClosedItemAction::Done,
            diagnostics: StatusTransitionDiagnostics {
                futuro_renamed_project: None,
                evicted_ids: vec![],
            },
        })
    }

    fn cancel_item(&self, spec: CancelItemSpec) -> Result<ClosedItem, Self::Error> {
        let mut state = self.state.lock().unwrap();
        if state.fail {
            return Err(FakeStoreError);
        }
        state.cancel_specs.push(spec.clone());
        Ok(ClosedItem {
            id: "PWF-0007".to_string(),
            project: "pwf".to_string(),
            title: "ship it".to_string(),
            action: ClosedItemAction::Cancelled,
            diagnostics: StatusTransitionDiagnostics {
                futuro_renamed_project: None,
                evicted_ids: vec![],
            },
        })
    }

    fn reopen_item(&self, id: &str) -> Result<ReopenedItem, Self::Error> {
        let mut state = self.state.lock().unwrap();
        if state.fail {
            return Err(FakeStoreError);
        }
        state.reopen_ids.push(id.to_string());
        Ok(ReopenedItem {
            id: id.to_string(),
            project: "pwf".to_string(),
            already_active: false,
        })
    }
}

#[derive(Clone, cqrsy::Mediator)]
struct RecordingAddSender {
    #[handles(pwf_application::AddPendingWorkItem)]
    handler: RecordingAddHandler,
}

#[derive(Clone, Default)]
struct RecordingAddHandler {
    state: Arc<Mutex<RecordingAddState>>,
}

#[derive(Default)]
struct RecordingAddState {
    commands: Vec<AddPendingWorkItem>,
    fail: bool,
}

impl RecordingAddSender {
    fn default() -> Self {
        Self {
            handler: RecordingAddHandler::default(),
        }
    }

    fn failing() -> Self {
        let sender = Self::default();
        sender.handler.state.lock().unwrap().fail = true;
        sender
    }

    fn commands(&self) -> Vec<AddPendingWorkItem> {
        self.handler.state.lock().unwrap().commands.clone()
    }
}

impl cqrsy::Handler<AddPendingWorkItem> for RecordingAddHandler {
    async fn handle(&self, req: AddPendingWorkItem) -> Result<AddedItem, AddPendingWorkError> {
        let mut state = self.state.lock().unwrap();
        if state.fail {
            return Err(AddPendingWorkError::WriteStore(Box::new(FakeStoreError)));
        }
        state.commands.push(req);
        Ok(AddedItem {
            id: "PWF-0008".to_string(),
            project: "pwf".to_string(),
            title: "review pwf-0007".to_string(),
            note_path: PathBuf::from("/notes/pwf/PWF-0008.md"),
            created_section: None,
        })
    }
}

#[test]
fn add_command_forwards_spec_to_write_store() {
    let store = RecordingWriteStore::default();
    let handler = AddPendingWorkItemHandler::new(store.clone());
    let tags = Tags::parse_values(&["SQLite,csharp-export".to_string()]).unwrap();

    let got = send_now(
        &(),
        &handler,
        AddPendingWorkItem {
            project_name: "pwf".to_string(),
            prompt: "Ship it /d tests pass".to_string(),
            title: Some("Ship It".to_string()),
            created: "2026-07-07".to_string(),
            section: Some("Human".to_string()),
            prereq: Some("[[PWF-0001]]".to_string()),
            effort: Some(2),
            tags: Some(tags.clone()),
        },
    )
    .unwrap();

    assert_eq!(got.id, "PWF-0007");
    assert_eq!(
        store.add_specs(),
        vec![AddItemSpec {
            project_name: "pwf".to_string(),
            prompt: "Ship it /d tests pass".to_string(),
            title: Some("Ship It".to_string()),
            created: "2026-07-07".to_string(),
            section: Some("Human".to_string()),
            prereq: Some("[[PWF-0001]]".to_string()),
            effort: Some(2),
            tags: Some(tags.clone()),
        }]
    );
    assert_eq!(store.add_specs()[0].tags, Some(tags));
}

#[test]
fn add_command_maps_store_error() {
    let handler = AddPendingWorkItemHandler::new(RecordingWriteStore::failing());

    let err = send_now(
        &(),
        &handler,
        AddPendingWorkItem {
            project_name: "pwf".to_string(),
            prompt: "Ship it".to_string(),
            title: None,
            created: "2026-07-07".to_string(),
            section: None,
            prereq: None,
            effort: None,
            tags: None,
        },
    )
    .unwrap_err();

    assert!(matches!(err, AddPendingWorkError::WriteStore(_)));
    assert_eq!(err.to_string(), "fake store failed");
}

#[test]
fn update_command_forwards_spec_to_write_store() {
    let store = RecordingWriteStore::default();
    let handler = UpdatePendingWorkItemHandler::new(store.clone());
    let tags = Tags::parse_values(&["SQLite,csharp-export".to_string()]).unwrap();

    let got = send_now(
        &(),
        &handler,
        UpdatePendingWorkItem {
            id: "pwf-7".to_string(),
            prompt: Some("new prompt".to_string()),
            title: Some("Renamed".to_string()),
            append: Some("extra goal".to_string()),
            prereq: vec!["PWF-0001".to_string()],
            clear_prereq: false,
            commits: Some("a..b".to_string()),
            append_report: Some("done".to_string()),
            effort: Some(3),
            tags: Some(tags.clone()),
            tags_clear: true,
        },
    )
    .unwrap();

    assert_eq!(
        got,
        UpdatedItem::OpenItemEdit {
            id: "PWF-0007".to_string(),
            project: "pwf".to_string(),
            title: "renamed".to_string(),
        }
    );
    assert_eq!(
        store.update_specs(),
        vec![UpdateItemSpec {
            id: "pwf-7".to_string(),
            prompt: Some("new prompt".to_string()),
            title: Some("Renamed".to_string()),
            append: Some("extra goal".to_string()),
            prereq: vec!["PWF-0001".to_string()],
            clear_prereq: false,
            commits: Some("a..b".to_string()),
            append_report: Some("done".to_string()),
            effort: Some(3),
            tags: Some(tags.clone()),
            tags_clear: true,
        }]
    );
    assert_eq!(store.update_specs()[0].tags, Some(tags));
    assert!(store.update_specs()[0].tags_clear);
}

#[test]
fn update_command_maps_store_error() {
    let handler = UpdatePendingWorkItemHandler::new(RecordingWriteStore::failing());

    let err = send_now(
        &(),
        &handler,
        UpdatePendingWorkItem {
            id: "PWF-0007".to_string(),
            prompt: Some("new prompt".to_string()),
            title: None,
            append: None,
            prereq: vec![],
            clear_prereq: false,
            commits: None,
            append_report: None,
            effort: None,
            tags: None,
            tags_clear: false,
        },
    )
    .unwrap_err();

    assert!(matches!(err, UpdatePendingWorkError::WriteStore(_)));
    assert_eq!(err.to_string(), "fake store failed");
}

#[test]
fn remove_command_forwards_id_to_write_store() {
    let store = RecordingWriteStore::default();
    let handler = RemovePendingWorkItemHandler::new(store.clone());

    let got = send_now(
        &(),
        &handler,
        RemovePendingWorkItem {
            id: "PWF-0007".to_string(),
        },
    )
    .unwrap();

    assert_eq!(got.id, "PWF-0007");
    assert_eq!(store.remove_ids(), vec!["PWF-0007".to_string()]);
}

#[test]
fn remove_command_maps_store_error() {
    let handler = RemovePendingWorkItemHandler::new(RecordingWriteStore::failing());

    let err = send_now(
        &(),
        &handler,
        RemovePendingWorkItem {
            id: "PWF-0007".to_string(),
        },
    )
    .unwrap_err();

    assert!(matches!(err, RemovePendingWorkError::WriteStore(_)));
    assert_eq!(err.to_string(), "fake store failed");
}

#[test]
fn complete_command_forwards_spec_to_write_store() {
    let store = RecordingWriteStore::default();
    let add_sender = RecordingAddSender::default();
    let handler = CompletePendingWorkHandler::new(store.clone(), add_sender);

    let got = send_now(
        &(),
        &handler,
        CompletePendingWork {
            id: "PWF-0007".to_string(),
            completed: "2026-07-07".to_string(),
            report: Some("landed cleanly".to_string()),
            commits: vec!["a..b,c..d".to_string(), "a..b".to_string()],
            review: false,
        },
    )
    .unwrap();

    assert_eq!(got.text, "Done PWF-0007 (pwf :: ship it)\n");
    assert_eq!(
        store.complete_specs(),
        vec![CompleteItemSpec {
            id: "PWF-0007".to_string(),
            completed: "2026-07-07".to_string(),
            report: Some("landed cleanly".to_string()),
            commits: Some("a..b, c..d".to_string()),
        }]
    );
}

#[test]
fn complete_command_creates_review_task_through_add_sender() {
    let store = RecordingWriteStore::default();
    let add_sender = RecordingAddSender::default();
    let handler = CompletePendingWorkHandler::new(store, add_sender.clone());

    let got = send_now(
        &(),
        &handler,
        CompletePendingWork {
            id: "pwf-7".to_string(),
            completed: "2026-07-07".to_string(),
            report: None,
            commits: vec!["a..b".to_string()],
            review: true,
        },
    )
    .unwrap();

    assert_eq!(
        got.text,
        "Done PWF-0007 (pwf :: ship it)\nADDED PWF TASK [PWF-0008] pwf :: review pwf-0007\n  file: /notes/pwf/PWF-0008.md\n"
    );
    let commands = add_sender.commands();
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].project_name, "pwf");
    assert_eq!(
        commands[0].prompt,
        "review PWF-0007 & git-tools diff a..b & git-tools diff-subrepos"
    );
    assert_eq!(commands[0].title, None);
    assert_eq!(commands[0].created, "2026-07-07");
    assert_eq!(commands[0].section, Some("Human".to_string()));
    assert_eq!(commands[0].prereq, None);
    assert_eq!(commands[0].effort, None);
}

#[test]
fn cancel_command_review_uses_canonical_id_and_bare_diff_without_commits() {
    let store = RecordingWriteStore::default();
    let add_sender = RecordingAddSender::default();
    let handler = CancelPendingWorkHandler::new(store, add_sender.clone());

    send_now(
        &(),
        &handler,
        CancelPendingWork::new(
            "pwf-7".to_string(),
            "2026-07-07".to_string(),
            "blocked by upstream".to_string(),
            vec![],
            true,
        )
        .unwrap(),
    )
    .unwrap();

    let commands = add_sender.commands();
    assert_eq!(commands.len(), 1);
    assert_eq!(
        commands[0].prompt,
        "review PWF-0007 & git-tools diff & git-tools diff-subrepos"
    );
}

#[test]
fn complete_command_maps_store_and_review_add_errors() {
    let failing_store_handler = CompletePendingWorkHandler::new(
        RecordingWriteStore::failing(),
        RecordingAddSender::default(),
    );

    let err = send_now(
        &(),
        &failing_store_handler,
        CompletePendingWork {
            id: "PWF-0007".to_string(),
            completed: "2026-07-07".to_string(),
            report: None,
            commits: vec![],
            review: false,
        },
    )
    .unwrap_err();

    assert!(matches!(err, CompletePendingWorkError::WriteStore(_)));
    assert_eq!(err.to_string(), "fake store failed");

    let failing_review_handler = CompletePendingWorkHandler::new(
        RecordingWriteStore::default(),
        RecordingAddSender::failing(),
    );

    let err = send_now(
        &(),
        &failing_review_handler,
        CompletePendingWork {
            id: "PWF-0007".to_string(),
            completed: "2026-07-07".to_string(),
            report: None,
            commits: vec![],
            review: true,
        },
    )
    .unwrap_err();

    assert!(matches!(err, CompletePendingWorkError::ReviewTask(_)));
    assert_eq!(err.to_string(), "fake store failed");
}

#[test]
fn cancel_command_requires_non_empty_report_before_send() {
    let err = CancelPendingWork::new(
        "PWF-0007".to_string(),
        "2026-07-07".to_string(),
        " \t\n".to_string(),
        vec![],
        false,
    )
    .unwrap_err();

    assert!(matches!(err, CancelPendingWorkError::EmptyReport));
    assert_eq!(err.to_string(), "--report cannot be empty.");
}

#[test]
fn cancel_command_forwards_spec_and_can_create_review_task() {
    let store = RecordingWriteStore::default();
    let add_sender = RecordingAddSender::default();
    let handler = CancelPendingWorkHandler::new(store.clone(), add_sender.clone());

    let got = send_now(
        &(),
        &handler,
        CancelPendingWork::new(
            "PWF-0007".to_string(),
            "2026-07-07".to_string(),
            "blocked by upstream".to_string(),
            vec!["a..b".to_string()],
            true,
        )
        .unwrap(),
    )
    .unwrap();

    assert_eq!(
        got.text,
        "Cancelled PWF-0007 (pwf :: ship it)\nADDED PWF TASK [PWF-0008] pwf :: review pwf-0007\n  file: /notes/pwf/PWF-0008.md\n"
    );
    assert_eq!(
        store.cancel_specs(),
        vec![CancelItemSpec {
            id: "PWF-0007".to_string(),
            completed: "2026-07-07".to_string(),
            report: "blocked by upstream".to_string(),
            commits: Some("a..b".to_string()),
        }]
    );
    assert_eq!(
        add_sender.commands()[0].prompt,
        "review PWF-0007 & git-tools diff a..b & git-tools diff-subrepos"
    );
}

#[test]
fn reopen_command_forwards_id_and_renders_skip_text() {
    let store = RecordingWriteStore::default();
    let handler = ReopenPendingWorkHandler::new(store.clone());

    let got = send_now(
        &(),
        &handler,
        ReopenPendingWork {
            id: "PWF-0007".to_string(),
        },
    )
    .unwrap();

    assert_eq!(got, "Reopened PWF-0007 (pwf)\n");
    assert_eq!(store.reopen_ids(), vec!["PWF-0007".to_string()]);

    let already = ReopenedItem {
        id: "PWF-0007".to_string(),
        project: "pwf".to_string(),
        already_active: true,
    };
    assert_eq!(
        already.to_output_text(),
        "PWF-0007 already active (pwf) — skipped\n"
    );
}

#[test]
fn reopen_command_maps_store_error() {
    let handler = ReopenPendingWorkHandler::new(RecordingWriteStore::failing());

    let err = send_now(
        &(),
        &handler,
        ReopenPendingWork {
            id: "PWF-0007".to_string(),
        },
    )
    .unwrap_err();

    assert!(matches!(err, ReopenPendingWorkError::WriteStore(_)));
    assert_eq!(err.to_string(), "fake store failed");
}
