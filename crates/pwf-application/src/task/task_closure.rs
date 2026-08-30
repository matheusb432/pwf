//! Applies the shared task completion and cancellation transition.

use pwf_models::{
    project::ProjectId,
    task::{CommitRanges, TaskId, TaskPrompt, TaskReport, TaskSection, TaskStatus, TaskTimestamp},
};
use pwf_wire::task::{AddTaskDiagnostics, ClosedTaskAction};

use crate::{
    ports::task_record::{
        IndexEntry, IndexEntryState, IndexEntryStore, IndexSectionStore, Materialization, NewTask,
        NullablePatch, TaskPatch, TaskStore,
    },
    task::{
        add_task::AddTaskError,
        infer_task_title,
        lane_configuration::TaskPromptLanes,
        note_body::append_report,
        task_body_region,
        task_creation::{self, TaskCreation},
    },
};

#[derive(Debug, thiserror::Error)]
pub enum CloseTaskError {
    #[error("Active task not found: {id}")]
    TaskNotFound { id: TaskId },
    #[error("Unknown project ID `{project_id}` for task {task_id}")]
    UnknownProjectId {
        task_id: TaskId,
        project_id: ProjectId,
    },
    #[error(transparent)]
    Revision(#[from] super::TaskRevisionConflict),
    #[error(transparent)]
    WriteStore(anyhow::Error),
    #[error("{0}")]
    ReviewTask(#[source] Box<AddTaskError>),
}

mod queue {
    use std::collections::BTreeMap;

    use pwf_models::task::{TaskId, TaskSection, TaskTimestamp};

    use crate::{
        ports::task_record::{IndexEntry, IndexEntryState, TaskRecord},
        task::section_alias,
    };

    const SECTION_CAPS: &[(&str, usize)] =
        &[("General", 6), ("Low-prio", 3), ("Future", 3), ("Human", 3)];

    pub(super) struct CloseDecisions {
        pub(super) evicted_ids: Vec<TaskId>,
        pub(super) normalize_futuro_header: bool,
        pub(super) mark_target: bool,
    }

    pub(super) fn apply_task_completion_timestamps(
        entries: &mut [IndexEntry],
        tasks: &[TaskRecord],
    ) {
        let timestamps = tasks
            .iter()
            .filter_map(|task| {
                task.completed_at
                    .map(|completed_at| (task.id.clone(), completed_at))
            })
            .collect::<BTreeMap<_, _>>();
        for entry in entries {
            entry.state = completion_state(&entry.state, timestamps.get(&entry.id));
        }
    }

    fn completion_state(
        state: &IndexEntryState,
        completed_at: Option<&TaskTimestamp>,
    ) -> IndexEntryState {
        match (state, completed_at) {
            (IndexEntryState::Done(_), Some(completed_at)) => {
                IndexEntryState::Done(Some(*completed_at))
            }
            (state, _) => state.clone(),
        }
    }

    pub(super) fn close_decisions(
        entries: &[IndexEntry],
        sections: &[TaskSection],
        id: &TaskId,
        completed_at: TaskTimestamp,
    ) -> CloseDecisions {
        let normalize_futuro_header = sections.iter().any(is_futuro_label);

        let Some(target) = entries
            .iter()
            .find(|entry| &entry.id == id && entry.state == IndexEntryState::Open)
        else {
            return CloseDecisions {
                evicted_ids: Vec::new(),
                normalize_futuro_header,
                mark_target: false,
            };
        };

        CloseDecisions {
            evicted_ids: evict_beyond_cap(entries, target.section.as_ref(), id, completed_at),
            normalize_futuro_header,
            mark_target: true,
        }
    }

    fn evict_beyond_cap(
        entries: &[IndexEntry],
        target_section: Option<&TaskSection>,
        id: &TaskId,
        completed_at: TaskTimestamp,
    ) -> Vec<TaskId> {
        let target_section = normalize_section(target_section);
        let Some(cap) = section_cap(&target_section) else {
            return Vec::new();
        };

        let mut done: Vec<(Option<TaskTimestamp>, &TaskId)> = entries
            .iter()
            .filter_map(|entry| match &entry.state {
                IndexEntryState::Done(entry_completed)
                    if normalize_section(entry.section.as_ref()) == target_section =>
                {
                    Some((*entry_completed, &entry.id))
                }
                IndexEntryState::Open | IndexEntryState::Done(_) => None,
            })
            .collect();
        done.push((Some(completed_at), id));

        if done.len() <= cap {
            return Vec::new();
        }

        let evict_count = done.len() - cap;
        done.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(right.1)));
        done.into_iter()
            .take(evict_count)
            .map(|(_, evicted_id)| evicted_id.clone())
            .collect()
    }

    pub(super) fn is_futuro_label(label: &TaskSection) -> bool {
        label.as_ref().eq_ignore_ascii_case("futuro")
    }

    fn section_cap(normalized_section: &str) -> Option<usize> {
        SECTION_CAPS
            .iter()
            .find(|(name, _)| *name == normalized_section)
            .map(|(_, cap)| *cap)
    }

    fn normalize_section(label: Option<&TaskSection>) -> String {
        let Some(label) = label else {
            return "General".to_string();
        };
        if label.as_ref() == "General" {
            return "General".to_string();
        }
        section_alias(label.as_ref()).map_or_else(|| label.as_ref().to_lowercase(), str::to_string)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::testing::task_timestamp;

        fn id(raw: &str) -> TaskId {
            TaskId::try_new(raw).unwrap()
        }

        fn done(raw_id: &str, completed_at: &str, section: &str) -> IndexEntry {
            IndexEntry {
                id: id(raw_id),
                state: IndexEntryState::Done(
                    (!completed_at.is_empty()).then(|| task_timestamp(completed_at)),
                ),
                section: (!section.is_empty()).then(|| section.parse().unwrap()),
            }
        }

        fn numbered_done(number: u16, section: &str) -> IndexEntry {
            done(
                &format!("FOO-{number:04}"),
                &format!("2026-01-{number:02}T00:00:00Z"),
                section,
            )
        }

        fn open(raw_id: &str, section: &str) -> IndexEntry {
            IndexEntry {
                id: id(raw_id),
                state: IndexEntryState::Open,
                section: (!section.is_empty()).then(|| section.parse().unwrap()),
            }
        }

        #[test]
        fn cap_boundary_evicts_single_oldest_beyond_cap() {
            let mut entries: Vec<IndexEntry> =
                (1..=6).map(|number| numbered_done(number, "")).collect();
            entries.push(open("FOO-0007", ""));

            let decisions = close_decisions(
                &entries,
                &[],
                &id("FOO-0007"),
                task_timestamp("2026-07-07T12:34:56Z"),
            );

            assert_eq!(decisions.evicted_ids, vec![id("FOO-0001")]);
            assert!(decisions.mark_target);
        }

        #[test]
        fn tied_completion_timestamps_break_by_ascending_id() {
            let entries = vec![
                done("FOO-0002", "2026-01-01T00:00:00Z", "Human"),
                done("FOO-0001", "2026-01-01T00:00:00Z", "Human"),
                done("FOO-0003", "2026-01-02T00:00:00Z", "Human"),
                open("FOO-0004", "Human"),
            ];

            let decisions = close_decisions(
                &entries,
                &[],
                &id("FOO-0004"),
                task_timestamp("2026-01-03T12:34:56Z"),
            );

            assert_eq!(decisions.evicted_ids, vec![id("FOO-0001")]);
        }

        #[test]
        fn missing_completion_timestamp_sorts_before_any_timestamped_entry() {
            let entries = vec![
                done("FOO-0001", "", "Human"),
                done("FOO-0002", "2026-01-01T00:00:00Z", "Human"),
                done("FOO-0003", "2026-01-02T00:00:00Z", "Human"),
                open("FOO-0004", "Human"),
            ];

            let decisions = close_decisions(
                &entries,
                &[],
                &id("FOO-0004"),
                task_timestamp("2026-01-03T12:34:56Z"),
            );

            assert_eq!(decisions.evicted_ids, vec![id("FOO-0001")]);
        }

        #[test]
        fn section_without_a_cap_evicts_nothing() {
            let mut entries: Vec<IndexEntry> = (1..=9)
                .map(|number| numbered_done(number, "Someday"))
                .collect();
            entries.push(open("FOO-0010", "Someday"));

            let decisions = close_decisions(
                &entries,
                &[],
                &id("FOO-0010"),
                task_timestamp("2026-07-07T12:34:56Z"),
            );

            assert!(decisions.evicted_ids.is_empty());
        }

        #[test]
        fn raw_section_label_aliases_before_cap_lookup() {
            let mut entries: Vec<IndexEntry> = (1..=3)
                .map(|number| numbered_done(number, "futuro"))
                .collect();
            entries.push(open("FOO-0004", "Futuro"));

            let decisions = close_decisions(
                &entries,
                &[],
                &id("FOO-0004"),
                task_timestamp("2026-07-07T12:34:56Z"),
            );

            assert_eq!(decisions.evicted_ids, vec![id("FOO-0001")]);
        }

        #[test]
        fn futuro_header_normalizes_when_target_entry_is_missing() {
            let sections = ["Futuro".parse().unwrap()];

            let decisions = close_decisions(
                &[],
                &sections,
                &id("FOO-0001"),
                task_timestamp("2026-07-07T12:34:56Z"),
            );

            assert!(decisions.normalize_futuro_header);
            assert!(!decisions.mark_target);
            assert!(decisions.evicted_ids.is_empty());
        }

        #[test]
        fn futuro_header_check_is_case_insensitive_and_trims_whitespace() {
            let sections = ["  FUTURO  ".parse().unwrap()];

            let decisions = close_decisions(
                &[],
                &sections,
                &id("FOO-0001"),
                task_timestamp("2026-07-07T12:34:56Z"),
            );

            assert!(decisions.normalize_futuro_header);
        }

        #[test]
        fn unrelated_headers_do_not_normalize() {
            let sections = [TaskSection::human(), TaskSection::future()];

            let decisions = close_decisions(
                &[],
                &sections,
                &id("FOO-0001"),
                task_timestamp("2026-07-07T12:34:56Z"),
            );

            assert!(!decisions.normalize_futuro_header);
        }
    }
}

use queue::{apply_task_completion_timestamps, close_decisions, is_futuro_label};
pub(in crate::task) struct TaskClosure<'a> {
    pub(in crate::task) action: ClosedTaskAction,
    pub(in crate::task) id: &'a TaskId,
    pub(in crate::task) completed_at: TaskTimestamp,
    pub(in crate::task) report: Option<&'a TaskReport>,
    pub(in crate::task) commits: Option<&'a CommitRanges>,
    pub(in crate::task) review_lanes: Option<&'a TaskPromptLanes>,
    pub(in crate::task) expected_revision: Option<&'a pwf_wire::task::TaskRevision>,
}

/// Carries the review task identifier needed by the mutation response.
pub(in crate::task) struct ClosedTaskEffects {
    pub(in crate::task) review_task: Option<task_creation::CreatedTask>,
}

/// Closes a task through the flow shared by done and cancel.
///
/// One patch applies report and status fields. Only note-backed records rotate the queue,
/// because missing-note records have no file-backed queue entry.
pub(in crate::task) fn close(
    command: &TaskClosure<'_>,
    store: &(impl TaskStore + IndexEntryStore + IndexSectionStore),
    project: &pwf_models::project::Project,
) -> Result<ClosedTaskEffects, CloseTaskError> {
    let TaskClosure {
        action,
        id,
        completed_at,
        report,
        commits,
        review_lanes,
        expected_revision,
    } = *command;
    let task_identifier = id.clone();
    if &project.id != task_identifier.project_id() {
        return Err(CloseTaskError::UnknownProjectId {
            project_id: task_identifier.project_id().clone(),
            task_id: task_identifier,
        });
    }
    let record = TaskStore::get(store, project, &task_identifier)
        .map_err(|error| CloseTaskError::WriteStore(anyhow::Error::new(error)))?
        .ok_or_else(|| CloseTaskError::TaskNotFound {
            id: task_identifier.clone(),
        })?;
    super::ensure_task_revision(expected_revision, &record)?;
    if record.status != TaskStatus::Active {
        return Err(CloseTaskError::TaskNotFound {
            id: task_identifier,
        });
    }
    let mut patch = TaskPatch {
        status: Some(close_status(action)),
        completed_at: NullablePatch::Set(completed_at),
        ..TaskPatch::default()
    };
    if let Some(report) = report {
        let body = append_report(task_body_region(&record.body), report.as_ref());
        patch.body = Some(body);
    }
    if let Some(commits) = commits {
        patch.commits = NullablePatch::Set(commits.to_string());
    }
    TaskStore::update(store, project, &task_identifier, patch)
        .map_err(|error| CloseTaskError::WriteStore(anyhow::Error::new(error)))?;

    if matches!(record.materialization, Materialization::NoteFile) {
        rotate_done_queue(store, project, &task_identifier, completed_at)?;
    }

    let review_task = review_lanes
        .map(|lanes| {
            spawn_review(
                store,
                project,
                &task_identifier,
                completed_at,
                commits,
                lanes,
            )
        })
        .transpose()?;

    Ok(ClosedTaskEffects { review_task })
}

fn close_status(action: ClosedTaskAction) -> TaskStatus {
    match action {
        ClosedTaskAction::Done => TaskStatus::Done,
        ClosedTaskAction::Cancelled => TaskStatus::Cancelled,
    }
}

/// Applies header normalization, the closed entry, and cap-based evictions to the index.
fn rotate_done_queue(
    store: &(impl TaskStore + IndexEntryStore + IndexSectionStore),
    project: &pwf_models::project::Project,
    id: &TaskId,
    completed_at: TaskTimestamp,
) -> Result<(), CloseTaskError> {
    let mut entries = IndexEntryStore::list_index_entries(store, project)
        .map_err(|error| CloseTaskError::WriteStore(anyhow::Error::new(error)))?;
    let tasks = TaskStore::list(store, project)
        .map_err(|error| CloseTaskError::WriteStore(anyhow::Error::new(error)))?;
    apply_task_completion_timestamps(&mut entries, &tasks);
    let sections = IndexSectionStore::list_index_sections(store, project)
        .map_err(|error| CloseTaskError::WriteStore(anyhow::Error::new(error)))?;
    let decisions = close_decisions(&entries, &sections, id, completed_at);

    if decisions.normalize_futuro_header {
        rename_futuro_headers(store, project, &sections)?;
    }
    if decisions.mark_target {
        IndexEntryStore::upsert_index_entry(
            store,
            project,
            IndexEntry {
                id: id.clone(),
                state: IndexEntryState::Done(Some(completed_at)),
                section: None,
            },
        )
        .map_err(|error| CloseTaskError::WriteStore(anyhow::Error::new(error)))?;
    }
    for evicted in &decisions.evicted_ids {
        IndexEntryStore::delete_index_entry(store, project, evicted)
            .map_err(|error| CloseTaskError::WriteStore(anyhow::Error::new(error)))?;
    }
    Ok(())
}

/// Renames every `## Futuro` header to `## Future` through the section port.
fn rename_futuro_headers(
    store: &impl IndexSectionStore,
    project: &pwf_models::project::Project,
    sections: &[TaskSection],
) -> Result<(), CloseTaskError> {
    let future = TaskSection::future();
    for section in sections.iter().filter(|section| is_futuro_label(section)) {
        IndexSectionStore::rename_index_section(store, project, section, &future)
            .map_err(|error| CloseTaskError::WriteStore(anyhow::Error::new(error)))?;
    }
    Ok(())
}

fn spawn_review(
    store: &(impl TaskStore + IndexEntryStore + IndexSectionStore),
    project: &pwf_models::project::Project,
    reviewed: &TaskId,
    completed_at: TaskTimestamp,
    commits: Option<&CommitRanges>,
    lanes: &TaskPromptLanes,
) -> Result<task_creation::CreatedTask, CloseTaskError> {
    let prompt = review_task_prompt(reviewed, commits);
    let review_title = infer_task_title(&prompt, lanes)
        .map_err(AddTaskError::from)
        .map_err(|error| CloseTaskError::ReviewTask(Box::new(error)))?;
    let id = store.next_id(project).map_err(|source| {
        CloseTaskError::ReviewTask(Box::new(AddTaskError::AllocateTaskId {
            project: project.title.clone(),
            source: anyhow::Error::new(source),
        }))
    })?;
    task_creation::create(
        TaskCreation {
            project,
            id: &id,
            new: NewTask {
                title: review_title,
                body: super::note_body::render(&prompt, lanes),
                created_at: completed_at,
                section: Some(TaskSection::human()),
                blocked_by: None,
                effort: None,
                priority: None,
                tags: None,
            },
        },
        store,
    )
    .map_err(|source| {
        CloseTaskError::ReviewTask(Box::new(AddTaskError::WriteStore {
            diagnostics: AddTaskDiagnostics {
                project: project.title.clone(),
                created_section: source.created_section().map(|(_, section)| section.clone()),
            },
            source,
        }))
    })
}

pub(in crate::task) fn review_task_prompt(
    reviewed_id: &TaskId,
    commits: Option<&CommitRanges>,
) -> TaskPrompt {
    TaskPrompt::new(match commits {
        Some(commits) => format!("review {reviewed_id}, commits: {commits}"),
        None => format!("review {reviewed_id}"),
    })
}
