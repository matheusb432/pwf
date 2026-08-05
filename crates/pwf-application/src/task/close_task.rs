use pwf_models::{
    project::ProjectId,
    task::{ProjectName, TaskId, TaskStatus, Timestamp},
};

use crate::{
    ports::task_record::{
        IndexEntry, IndexEntryState, IndexEntryStore, IndexSection, IndexSectionStore,
        Materialization, NewTask, NullablePatch, TaskPatch, TaskStore,
    },
    task::{
        TaskSection,
        add_task::{AddTaskDiagnostics, AddTaskError, AddTaskOk},
        create_task::{self, CreateTask},
        created_task_output, infer_task_title, normalize_commit_ranges,
        note_body::append_report,
        task_body_region,
    },
};

/// Selects the status and confirmation verb recorded by a close interactor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClosedTaskAction {
    Done,
    Cancelled,
}

impl ClosedTaskAction {
    #[must_use]
    pub fn past_tense(self) -> &'static str {
        match self {
            Self::Done => "Done",
            Self::Cancelled => "Cancelled",
        }
    }

    fn status(self) -> TaskStatus {
        match self {
            Self::Done => TaskStatus::Done,
            Self::Cancelled => TaskStatus::Cancelled,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompleteTaskOk {
    pub id: TaskId,
    pub project: ProjectName,
    pub title: String,
    pub action: ClosedTaskAction,
    pub evicted_ids: Vec<TaskId>,
    pub futuro_renamed_project: Option<ProjectName>,
    pub review_task: Option<AddTaskOk>,
}

#[derive(Debug, thiserror::Error)]
pub enum CloseTaskError {
    #[error("Active task not found: {id}")]
    TaskNotFound { id: TaskId },
    #[error("Unknown project ID `{project_id}` for task {task_id}")]
    UnknownProjectId {
        task_id: TaskId,
        project_id: ProjectId,
    },
    #[error("--report cannot be empty.")]
    EmptyReport,
    #[error("{0}")]
    WriteStore(Box<dyn std::error::Error + Send + Sync>),
    #[error("{0}")]
    ReviewTask(#[source] AddTaskError),
}

mod queue {
    use pwf_models::task::{TaskId, Timestamp};

    use crate::{
        ports::task_record::{IndexEntry, IndexEntryState, IndexSection},
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
        sections: &[IndexSection],
        id: &TaskId,
        completed: &Timestamp,
    ) -> CloseDecisions {
        let normalize_futuro_header = sections
            .iter()
            .any(|section| is_futuro_label(&section.label));

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
            evicted_ids: evict_beyond_cap(entries, &target.section, id, completed),
            normalize_futuro_header,
            mark_target: true,
        }
    }

    fn evict_beyond_cap(
        entries: &[IndexEntry],
        target_section: &str,
        id: &TaskId,
        completed: &Timestamp,
    ) -> Vec<TaskId> {
        let target_section = normalize_section(target_section);
        let Some(cap) = section_cap(&target_section) else {
            return Vec::new();
        };

        let mut done: Vec<(&str, &TaskId)> = entries
            .iter()
            .filter_map(|entry| match &entry.state {
                IndexEntryState::Done(entry_completed)
                    if normalize_section(&entry.section) == target_section =>
                {
                    Some((entry_completed.as_str(), &entry.id))
                }
                IndexEntryState::Open | IndexEntryState::Done(_) => None,
            })
            .collect();
        done.push((completed.as_str(), id));

        if done.len() <= cap {
            return Vec::new();
        }

        let evict_count = done.len() - cap;
        done.sort_by(|left, right| left.0.cmp(right.0).then(left.1.cmp(right.1)));
        done.into_iter()
            .take(evict_count)
            .map(|(_, evicted_id)| evicted_id.clone())
            .collect()
    }

    pub(super) fn is_futuro_label(label: &str) -> bool {
        label.trim().eq_ignore_ascii_case("futuro")
    }

    fn section_cap(normalized_section: &str) -> Option<usize> {
        SECTION_CAPS
            .iter()
            .find(|(name, _)| *name == normalized_section)
            .map(|(_, cap)| *cap)
    }

    fn normalize_section(label: &str) -> String {
        if label.trim().is_empty() || label.trim() == "General" {
            return "General".to_string();
        }
        section_alias(label).map_or_else(|| label.trim().to_lowercase(), str::to_string)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn id(raw: &str) -> TaskId {
            TaskId::try_new(raw).unwrap()
        }

        fn done(raw_id: &str, date: &str, section: &str) -> IndexEntry {
            IndexEntry {
                id: id(raw_id),
                state: IndexEntryState::Done(Timestamp::new(date)),
                section: section.to_string(),
            }
        }

        fn open(raw_id: &str, section: &str) -> IndexEntry {
            IndexEntry {
                id: id(raw_id),
                state: IndexEntryState::Open,
                section: section.to_string(),
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

            let decisions = close_decisions(
                &entries,
                &[],
                &id("PWF-0007"),
                &Timestamp::new("2026-07-07"),
            );

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

            let decisions = close_decisions(
                &entries,
                &[],
                &id("PWF-0004"),
                &Timestamp::new("2026-01-03"),
            );

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

            let decisions = close_decisions(
                &entries,
                &[],
                &id("PWF-0004"),
                &Timestamp::new("2026-01-03"),
            );

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

            let decisions = close_decisions(
                &entries,
                &[],
                &id("PWF-0010"),
                &Timestamp::new("2026-07-07"),
            );

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

            let decisions = close_decisions(
                &entries,
                &[],
                &id("PWF-0004"),
                &Timestamp::new("2026-07-07"),
            );

            assert_eq!(decisions.evicted_ids, vec![id("PWF-0001")]);
        }

        #[test]
        fn futuro_header_normalizes_when_target_entry_is_missing() {
            let sections = [IndexSection {
                label: "Futuro".to_string(),
            }];

            let decisions = close_decisions(
                &[],
                &sections,
                &id("PWF-0001"),
                &Timestamp::new("2026-07-07"),
            );

            assert!(decisions.normalize_futuro_header);
            assert!(!decisions.mark_target);
            assert!(decisions.evicted_ids.is_empty());
        }

        #[test]
        fn futuro_header_check_is_case_insensitive_and_trims_whitespace() {
            let sections = [IndexSection {
                label: "  FUTURO  ".to_string(),
            }];

            let decisions = close_decisions(
                &[],
                &sections,
                &id("PWF-0001"),
                &Timestamp::new("2026-07-07"),
            );

            assert!(decisions.normalize_futuro_header);
        }

        #[test]
        fn unrelated_headers_do_not_normalize() {
            let sections = [
                IndexSection {
                    label: "Human".to_string(),
                },
                IndexSection {
                    label: "Future".to_string(),
                },
            ];

            let decisions = close_decisions(
                &[],
                &sections,
                &id("PWF-0001"),
                &Timestamp::new("2026-07-07"),
            );

            assert!(!decisions.normalize_futuro_header);
        }
    }
}

use queue::{close_decisions, is_futuro_label};
pub(in crate::task) struct CloseTask<'a> {
    pub(in crate::task) action: ClosedTaskAction,
    pub(in crate::task) id: &'a TaskId,
    pub(in crate::task) completed: Timestamp,
    pub(in crate::task) report: Option<&'a str>,
    pub(in crate::task) commits: &'a [String],
    pub(in crate::task) review: bool,
}

/// Closes an task through the flow shared by done and cancel.
///
/// One patch applies report and status fields. Only note-backed records rotate the queue,
/// because missing-note records have no file-backed queue entry.
pub(in crate::task) fn execute(
    command: CloseTask<'_>,
    store: &(impl TaskStore + IndexEntryStore + IndexSectionStore),
    project: &pwf_models::project::Project,
) -> Result<CompleteTaskOk, CloseTaskError> {
    let CloseTask {
        action,
        id,
        completed,
        report,
        commits,
        review,
    } = command;
    let commits_value = normalize_commit_ranges(commits);
    let task_identifier = id.clone();
    if project.id != task_identifier.project_id() {
        return Err(CloseTaskError::UnknownProjectId {
            project_id: task_identifier.project_id(),
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
    let title = record.title.clone();
    let mut patch = TaskPatch {
        status: Some(action.status()),
        completed: NullablePatch::Set(completed.clone()),
        ..TaskPatch::default()
    };
    if let Some(report) = report {
        let body = append_report(task_body_region(&record.body), report)
            .ok_or(CloseTaskError::EmptyReport)?;
        patch.body = Some(body);
    }
    if let Some(commits) = &commits_value {
        patch.commits = NullablePatch::Set(commits.clone());
    }
    TaskStore::update(store, project, &task_identifier, patch)
        .map_err(|error| CloseTaskError::WriteStore(Box::new(error)))?;

    let (evicted_ids, futuro_renamed) =
        if matches!(record.materialization, Materialization::NoteFile) {
            rotate_done_queue(store, project, &task_identifier, completed.as_str())?
        } else {
            (Vec::new(), false)
        };

    let review_task = review
        .then(|| {
            spawn_review(
                store,
                project,
                &task_identifier,
                completed.as_str(),
                commits_value.as_deref(),
            )
        })
        .transpose()?;

    Ok(CompleteTaskOk {
        id: task_identifier,
        project: project.title.clone(),
        title,
        action,
        evicted_ids,
        futuro_renamed_project: futuro_renamed.then(|| project.title.clone()),
        review_task,
    })
}

/// Applies header normalization, the closed entry, and cap-based evictions to the index.
fn rotate_done_queue(
    store: &(impl IndexEntryStore + IndexSectionStore),
    project: &pwf_models::project::Project,
    id: &TaskId,
    completed: &str,
) -> Result<(Vec<TaskId>, bool), CloseTaskError> {
    let entries = IndexEntryStore::list_index_entries(store, project)
        .map_err(|error| CloseTaskError::WriteStore(Box::new(error)))?;
    let sections = IndexSectionStore::list_index_sections(store, project)
        .map_err(|error| CloseTaskError::WriteStore(Box::new(error)))?;
    let completed = Timestamp::new(completed);
    let decisions = close_decisions(&entries, &sections, id, &completed);

    if decisions.normalize_futuro_header {
        rename_futuro_headers(store, project, &sections)?;
    }
    if decisions.mark_target {
        IndexEntryStore::upsert_index_entry(
            store,
            project,
            IndexEntry {
                id: id.clone(),
                state: IndexEntryState::Done(completed),
                section: String::new(),
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
    sections: &[IndexSection],
) -> Result<(), CloseTaskError> {
    for section in sections.iter().filter(|s| is_futuro_label(&s.label)) {
        IndexSectionStore::rename_index_section(store, project, &section.label, "Future")
            .map_err(|error| CloseTaskError::WriteStore(Box::new(error)))?;
    }
    Ok(())
}

fn spawn_review(
    store: &(impl TaskStore + IndexEntryStore + IndexSectionStore),
    project: &pwf_models::project::Project,
    reviewed: &TaskId,
    completed: &str,
    commits: Option<&str>,
) -> Result<AddTaskOk, CloseTaskError> {
    let prompt = review_task_prompt(reviewed, commits);
    let review_title = infer_task_title(&prompt)
        .map_err(AddTaskError::from)
        .map_err(CloseTaskError::ReviewTask)?;
    let created = create_task::execute(
        CreateTask {
            project,
            new: NewTask {
                title: review_title,
                body: super::note_body::render(&prompt),
                created: Timestamp::new(completed),
                section: Some(TaskSection::Human.as_str().to_string()),
                prereq: None,
                effort: None,
                tags: None,
            },
        },
        store,
    )
    .map_err(|source| {
        CloseTaskError::ReviewTask(AddTaskError::WriteStore {
            diagnostics: AddTaskDiagnostics {
                project: project.title.to_string(),
                created_section: source
                    .created_section()
                    .map(|(_, section)| section.to_string()),
            },
            source,
        })
    })?;
    Ok(created_task_output(project, created))
}

pub(in crate::task) fn review_task_prompt(reviewed_id: &TaskId, range: Option<&str>) -> String {
    let (title, diff) = match range {
        Some(range) => (
            format!("review {reviewed_id}, commits: {range}"),
            format!("git-tools diff {range}"),
        ),
        None => (
            format!("review {reviewed_id}"),
            "git-tools diff".to_string(),
        ),
    };
    format!("{title} / {diff} / git-tools diff-subrepos")
}
