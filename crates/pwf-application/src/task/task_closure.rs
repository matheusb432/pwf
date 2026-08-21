//! Applies the shared task completion and cancellation transition.

use pwf_models::{
    AppDate,
    project::ProjectId,
    task::{
        CommitRanges, TaskId, TaskPrompt, TaskReport, TaskSection, TaskStatus, TaskTitle,
        TaskTitleError,
    },
};

use crate::{
    contract::task::{AddTaskDiagnostics, AddedTask, ClosedTask, ClosedTaskAction},
    ports::task_record::{
        IndexEntry, IndexEntryState, IndexEntryStore, IndexSectionStore, Materialization, NewTask,
        NullablePatch, TaskPatch, TaskStore,
    },
    task::{
        add_task::AddTaskError,
        created_task_output, infer_task_title,
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
    #[error("task {id} has an invalid persisted title: {source}")]
    InvalidTitle {
        id: TaskId,
        #[source]
        source: TaskTitleError,
    },
    #[error("{0}")]
    WriteStore(#[source] Box<dyn std::error::Error + Send + Sync>),
    #[error("{0}")]
    ReviewTask(#[source] Box<AddTaskError>),
}

mod queue {
    use pwf_models::{
        AppDate,
        task::{TaskId, TaskSection},
    };

    use crate::{
        ports::task_record::{IndexEntry, IndexEntryState},
        task::section_alias,
    };

    const SECTION_CAPS: &[(&str, usize)] =
        &[("General", 6), ("Low-prio", 3), ("Future", 3), ("Human", 3)];

    pub(super) struct CloseDecisions {
        pub(super) evicted_ids: Vec<TaskId>,
        pub(super) normalize_futuro_header: bool,
        pub(super) mark_target: bool,
    }

    pub(super) fn close_decisions(
        entries: &[IndexEntry],
        sections: &[TaskSection],
        id: &TaskId,
        completed: AppDate,
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
            evicted_ids: evict_beyond_cap(entries, target.section.as_ref(), id, completed),
            normalize_futuro_header,
            mark_target: true,
        }
    }

    fn evict_beyond_cap(
        entries: &[IndexEntry],
        target_section: Option<&TaskSection>,
        id: &TaskId,
        completed: AppDate,
    ) -> Vec<TaskId> {
        let target_section = normalize_section(target_section);
        let Some(cap) = section_cap(&target_section) else {
            return Vec::new();
        };

        let mut done: Vec<(Option<AppDate>, &TaskId)> = entries
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
        done.push((Some(completed), id));

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
        use crate::testing::app_date;

        fn id(raw: &str) -> TaskId {
            TaskId::try_new(raw).unwrap()
        }

        fn done(raw_id: &str, date: &str, section: &str) -> IndexEntry {
            IndexEntry {
                id: id(raw_id),
                state: IndexEntryState::Done((!date.is_empty()).then(|| app_date(date))),
                section: (!section.is_empty()).then(|| section.parse().unwrap()),
            }
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
            let mut entries: Vec<IndexEntry> = (1..=6)
                .map(|number| {
                    done(
                        &format!("PWF-{number:04}"),
                        &format!("2026-01-{number:02}"),
                        "",
                    )
                })
                .collect();
            entries.push(open("PWF-0007", ""));

            let decisions = close_decisions(&entries, &[], &id("PWF-0007"), app_date("2026-07-07"));

            assert_eq!(decisions.evicted_ids, vec![id("PWF-0001")]);
            assert!(decisions.mark_target);
        }

        #[test]
        fn tied_completed_dates_break_by_ascending_id() {
            let entries = vec![
                done("PWF-0002", "2026-01-01", "Human"),
                done("PWF-0001", "2026-01-01", "Human"),
                done("PWF-0003", "2026-01-02", "Human"),
                open("PWF-0004", "Human"),
            ];

            let decisions = close_decisions(&entries, &[], &id("PWF-0004"), app_date("2026-01-03"));

            assert_eq!(decisions.evicted_ids, vec![id("PWF-0001")]);
        }

        #[test]
        fn missing_completed_date_sorts_before_any_dated_entry() {
            let entries = vec![
                done("PWF-0001", "", "Human"),
                done("PWF-0002", "2026-01-01", "Human"),
                done("PWF-0003", "2026-01-02", "Human"),
                open("PWF-0004", "Human"),
            ];

            let decisions = close_decisions(&entries, &[], &id("PWF-0004"), app_date("2026-01-03"));

            assert_eq!(decisions.evicted_ids, vec![id("PWF-0001")]);
        }

        #[test]
        fn section_without_a_cap_evicts_nothing() {
            let mut entries: Vec<IndexEntry> = (1..=9)
                .map(|number| {
                    done(
                        &format!("PWF-{number:04}"),
                        &format!("2026-01-{number:02}"),
                        "Someday",
                    )
                })
                .collect();
            entries.push(open("PWF-0010", "Someday"));

            let decisions = close_decisions(&entries, &[], &id("PWF-0010"), app_date("2026-07-07"));

            assert!(decisions.evicted_ids.is_empty());
        }

        #[test]
        fn raw_section_label_aliases_before_cap_lookup() {
            let mut entries: Vec<IndexEntry> = (1..=3)
                .map(|number| {
                    done(
                        &format!("PWF-{number:04}"),
                        &format!("2026-01-{number:02}"),
                        "futuro",
                    )
                })
                .collect();
            entries.push(open("PWF-0004", "Futuro"));

            let decisions = close_decisions(&entries, &[], &id("PWF-0004"), app_date("2026-07-07"));

            assert_eq!(decisions.evicted_ids, vec![id("PWF-0001")]);
        }

        #[test]
        fn futuro_header_normalizes_when_target_entry_is_missing() {
            let sections = ["Futuro".parse().unwrap()];

            let decisions =
                close_decisions(&[], &sections, &id("PWF-0001"), app_date("2026-07-07"));

            assert!(decisions.normalize_futuro_header);
            assert!(!decisions.mark_target);
            assert!(decisions.evicted_ids.is_empty());
        }

        #[test]
        fn futuro_header_check_is_case_insensitive_and_trims_whitespace() {
            let sections = ["  FUTURO  ".parse().unwrap()];

            let decisions =
                close_decisions(&[], &sections, &id("PWF-0001"), app_date("2026-07-07"));

            assert!(decisions.normalize_futuro_header);
        }

        #[test]
        fn unrelated_headers_do_not_normalize() {
            let sections = [TaskSection::human(), TaskSection::future()];

            let decisions =
                close_decisions(&[], &sections, &id("PWF-0001"), app_date("2026-07-07"));

            assert!(!decisions.normalize_futuro_header);
        }
    }
}

use queue::{close_decisions, is_futuro_label};
pub(in crate::task) struct TaskClosure<'a> {
    pub(in crate::task) action: ClosedTaskAction,
    pub(in crate::task) id: &'a TaskId,
    pub(in crate::task) completed: AppDate,
    pub(in crate::task) report: Option<&'a TaskReport>,
    pub(in crate::task) commits: Option<&'a CommitRanges>,
    pub(in crate::task) review: bool,
}

/// Closes a task through the flow shared by done and cancel.
///
/// One patch applies report and status fields. Only note-backed records rotate the queue,
/// because missing-note records have no file-backed queue entry.
pub(in crate::task) fn close(
    command: &TaskClosure<'_>,
    store: &(impl TaskStore + IndexEntryStore + IndexSectionStore),
    project: &pwf_models::project::Project,
) -> Result<ClosedTask, CloseTaskError> {
    let TaskClosure {
        action,
        id,
        completed,
        report,
        commits,
        review,
    } = *command;
    let task_identifier = id.clone();
    if &project.id != task_identifier.project_id() {
        return Err(CloseTaskError::UnknownProjectId {
            project_id: task_identifier.project_id().clone(),
            task_id: task_identifier,
        });
    }
    let record = TaskStore::get(store, project, &task_identifier)
        .map_err(|error| CloseTaskError::WriteStore(Box::new(error)))?
        .ok_or_else(|| CloseTaskError::TaskNotFound {
            id: task_identifier.clone(),
        })?;
    if record.status != TaskStatus::Active {
        return Err(CloseTaskError::TaskNotFound {
            id: task_identifier,
        });
    }
    let title = TaskTitle::try_new(record.title.clone()).map_err(|source| {
        CloseTaskError::InvalidTitle {
            id: task_identifier.clone(),
            source,
        }
    })?;
    let mut patch = TaskPatch {
        status: Some(close_status(action)),
        completed: NullablePatch::Set(completed),
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
        .map_err(|error| CloseTaskError::WriteStore(Box::new(error)))?;

    let (evicted_ids, futuro_renamed) =
        if matches!(record.materialization, Materialization::NoteFile) {
            rotate_done_queue(store, project, &task_identifier, completed)?
        } else {
            (Vec::new(), false)
        };

    let review_task = review
        .then(|| spawn_review(store, project, &task_identifier, completed, commits))
        .transpose()?;

    Ok(ClosedTask {
        id: task_identifier,
        project: project.title.clone(),
        title,
        action,
        evicted_ids,
        futuro_renamed_project: futuro_renamed.then(|| project.title.clone()),
        review_task,
    })
}

fn close_status(action: ClosedTaskAction) -> TaskStatus {
    match action {
        ClosedTaskAction::Done => TaskStatus::Done,
        ClosedTaskAction::Cancelled => TaskStatus::Cancelled,
    }
}

/// Applies header normalization, the closed entry, and cap-based evictions to the index.
fn rotate_done_queue(
    store: &(impl IndexEntryStore + IndexSectionStore),
    project: &pwf_models::project::Project,
    id: &TaskId,
    completed: AppDate,
) -> Result<(Vec<TaskId>, bool), CloseTaskError> {
    let entries = IndexEntryStore::list_index_entries(store, project)
        .map_err(|error| CloseTaskError::WriteStore(Box::new(error)))?;
    let sections = IndexSectionStore::list_index_sections(store, project)
        .map_err(|error| CloseTaskError::WriteStore(Box::new(error)))?;
    let decisions = close_decisions(&entries, &sections, id, completed);

    if decisions.normalize_futuro_header {
        rename_futuro_headers(store, project, &sections)?;
    }
    if decisions.mark_target {
        IndexEntryStore::upsert_index_entry(
            store,
            project,
            IndexEntry {
                id: id.clone(),
                state: IndexEntryState::Done(Some(completed)),
                section: None,
            },
        )
        .map_err(|error| CloseTaskError::WriteStore(Box::new(error)))?;
    }
    for evicted in &decisions.evicted_ids {
        IndexEntryStore::delete_index_entry(store, project, evicted)
            .map_err(|error| CloseTaskError::WriteStore(Box::new(error)))?;
    }
    Ok((decisions.evicted_ids, decisions.normalize_futuro_header))
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
            .map_err(|error| CloseTaskError::WriteStore(Box::new(error)))?;
    }
    Ok(())
}

fn spawn_review(
    store: &(impl TaskStore + IndexEntryStore + IndexSectionStore),
    project: &pwf_models::project::Project,
    reviewed: &TaskId,
    completed: AppDate,
    commits: Option<&CommitRanges>,
) -> Result<AddedTask, CloseTaskError> {
    let prompt = review_task_prompt(reviewed, commits);
    let review_title = infer_task_title(&prompt)
        .map_err(AddTaskError::from)
        .map_err(|error| CloseTaskError::ReviewTask(Box::new(error)))?;
    let id = store.next_id(project).map_err(|source| {
        CloseTaskError::ReviewTask(Box::new(AddTaskError::AllocateTaskId {
            project: project.title.clone(),
            source: Box::new(source),
        }))
    })?;
    let created = task_creation::create(
        TaskCreation {
            project,
            id: &id,
            new: NewTask {
                title: review_title,
                body: super::note_body::render(&prompt),
                created: completed,
                section: Some(TaskSection::human()),
                blocked_by: None,
                effort: None,
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
    })?;
    Ok(created_task_output(project, created))
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
