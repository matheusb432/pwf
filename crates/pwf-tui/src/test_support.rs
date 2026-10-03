use pwf_models::{
    note::NoteId,
    project::ProjectId,
    revision::ContentRevision,
    task::{BlockedBy, EffortTier, PriorityTier, TaskStatus, TaskTags, TaskTitle},
};

use crate::{
    app::App,
    browser::{Project, ProjectScope, Record, RecordId, Snapshot},
    draft::TaskTarget,
};

pub(super) fn app() -> App {
    let mut app = App::new(ProjectScope::All);
    app.browser.replace(snapshot());
    app
}

pub(super) fn snapshot() -> Snapshot {
    let project = ProjectId::try_new("PWF").unwrap();
    let mut task = Record::new(
        RecordId::Task("pwf7".parse().unwrap()),
        project.clone(),
        "Build the picker".into(),
        "/tasks/PWF-0007.md".into(),
    );
    task.status = Some(TaskStatus::Active);
    task.set_body("## Goals\nSearch saved Markdown\n".into());
    let mut note = Record::new(
        RecordId::Note(NoteId::try_new("PWF-NOTE-0001").unwrap()),
        project.clone(),
        "Keyboard notes".into(),
        "/tasks/PWF-NOTE-0001.md".into(),
    );
    note.set_body("# Keyboard notes\nSearch saved Markdown and draft recovery".into());
    Snapshot {
        project: None,
        projects: vec![Project {
            id: project,
            title: "pwf".into(),
            tasks_path: "/tasks".into(),
        }],
        records: vec![task, note],
        warnings: Vec::new(),
    }
}

pub(super) fn task_target() -> TaskTarget {
    TaskTarget {
        id: "pwf7".parse().unwrap(),
        revision: ContentRevision::try_new("a".repeat(64)).unwrap(),
        title: TaskTitle::try_new("Build the picker").unwrap(),
        tags: TaskTags::from_inputs(&["rust, tui".parse().unwrap()]),
        priority: Some(PriorityTier::High),
        effort: Some(EffortTier::Low),
        blocked_by: Some(BlockedBy::try_new(["aux2".parse().unwrap()]).unwrap()),
    }
}
