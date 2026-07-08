use std::{
    error::Error,
    fmt,
    sync::{Arc, Mutex},
};

use cqrsy::send_now;
use pwf_application::{
    PendingWorkResolveStore, ResolvePendingWorkError, ResolvePendingWorkHandler,
    ResolvePendingWorkItem, ResolvePendingWorkOutput, ShowPendingWorkHandler, ShowPendingWorkItem,
};

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolveCall {
    id: String,
    show: bool,
}

#[derive(Clone, Default)]
struct RecordingResolveStore {
    state: Arc<Mutex<ResolveState>>,
}

#[derive(Default)]
struct ResolveState {
    calls: Vec<ResolveCall>,
    fail: bool,
}

#[derive(Debug, Clone)]
struct FakeStoreError;

impl fmt::Display for FakeStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("fake resolve store failed")
    }
}

impl Error for FakeStoreError {}

impl RecordingResolveStore {
    fn failing() -> Self {
        let store = Self::default();
        store.state.lock().unwrap().fail = true;
        store
    }

    fn calls(&self) -> Vec<ResolveCall> {
        self.state.lock().unwrap().calls.clone()
    }
}

impl PendingWorkResolveStore for RecordingResolveStore {
    type Error = FakeStoreError;

    fn resolve_item(&self, id: &str, show: bool) -> Result<ResolvePendingWorkOutput, Self::Error> {
        let mut state = self.state.lock().unwrap();
        if state.fail {
            return Err(FakeStoreError);
        }
        state.calls.push(ResolveCall {
            id: id.to_string(),
            show,
        });
        Ok(if show {
            ResolvePendingWorkOutput::NoteMarkdown(format!("body for {id}"))
        } else {
            ResolvePendingWorkOutput::NotePath(format!("/notes/{id}.md"))
        })
    }
}

#[test]
fn resolve_query_forwards_id_and_show_flag_to_store() {
    let store = RecordingResolveStore::default();
    let handler = ResolvePendingWorkHandler::new(store.clone());

    let got = send_now(
        &(),
        &handler,
        ResolvePendingWorkItem {
            id: "pwf-7".to_string(),
            show: false,
        },
    )
    .unwrap();

    assert_eq!(
        got,
        ResolvePendingWorkOutput::NotePath("/notes/pwf-7.md".to_string())
    );
    assert_eq!(
        store.calls(),
        vec![ResolveCall {
            id: "pwf-7".to_string(),
            show: false,
        }]
    );
}

#[test]
fn resolve_query_maps_store_error() {
    let handler = ResolvePendingWorkHandler::new(RecordingResolveStore::failing());

    let err = send_now(
        &(),
        &handler,
        ResolvePendingWorkItem {
            id: "PWF-0007".to_string(),
            show: true,
        },
    )
    .unwrap_err();

    assert!(matches!(err, ResolvePendingWorkError::ReadStore(_)));
    assert_eq!(err.to_string(), "fake resolve store failed");
}

#[test]
fn show_query_uses_markdown_resolution_path() {
    let store = RecordingResolveStore::default();
    let handler = ShowPendingWorkHandler::new(store.clone());

    let got = send_now(
        &(),
        &handler,
        ShowPendingWorkItem {
            id: "pwf-8".to_string(),
        },
    )
    .unwrap();

    assert_eq!(
        got,
        ResolvePendingWorkOutput::NoteMarkdown("body for pwf-8".to_string())
    );
    assert_eq!(
        store.calls(),
        vec![ResolveCall {
            id: "pwf-8".to_string(),
            show: true,
        }]
    );
}
