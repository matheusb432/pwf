use std::{
    collections::{VecDeque, hash_map::RandomState},
    hash::BuildHasher as _,
    mem::{size_of, size_of_val},
    sync::Mutex,
    time::{Duration, Instant},
};

use pwf_wire::{
    pagination::CursorPage,
    task::{BlockedByIssue, TaskIssue},
};

use super::{
    ListTasksError, ListedTask, PageCursor, TaskListPage, TaskPageSize, apply_page,
    encode_page_token,
};

const SNAPSHOT_COUNT_MAX: usize = 16;
const SNAPSHOT_BYTES_MAX: usize = 16 * 1024 * 1024;
const SNAPSHOT_LIFETIME: Duration = Duration::from_secs(60);

/// Bounds retained list payloads to 16 MiB; continuation tokens expire after 60 seconds.
#[derive(Default)]
pub struct ListTasksSnapshots {
    state: Mutex<SnapshotState>,
}

#[derive(Default)]
struct SnapshotState {
    entries: VecDeque<Snapshot>,
    identities: RandomState,
    sequence: u64,
}

struct Snapshot {
    id: String,
    binding: String,
    created: Instant,
    tasks: Box<[ListedTask]>,
    hidden: usize,
    bytes: usize,
}

impl ListTasksSnapshots {
    pub(super) fn first_page(
        &self,
        tasks: Vec<ListedTask>,
        hidden: usize,
        page_size: Option<TaskPageSize>,
        binding: &str,
    ) -> Result<TaskListPage, ListTasksError> {
        let Some(page_size) = page_size.filter(|size| tasks.len() > size.get()) else {
            return Ok(TaskListPage {
                page: CursorPage {
                    items: tasks,
                    next_key: None,
                },
                hidden,
            });
        };
        let bytes = snapshot_bytes(&tasks);
        if bytes > SNAPSHOT_BYTES_MAX {
            let page = apply_page(tasks, Some(page_size), None, binding)?;
            return Ok(TaskListPage { page, hidden });
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| ListTasksError::SnapshotUnavailable)?;
        let now = Instant::now();
        state.prune(now);
        while state.entries.len() >= SNAPSHOT_COUNT_MAX
            || state.entries.iter().map(|entry| entry.bytes).sum::<usize>() + bytes
                > SNAPSHOT_BYTES_MAX
        {
            state.entries.pop_front();
        }
        state.sequence = state.sequence.wrapping_add(1);
        let id = format!("{:016x}", state.identities.hash_one(state.sequence));
        let snapshot = Snapshot {
            id,
            binding: binding.to_string(),
            created: now,
            tasks: tasks.into_boxed_slice(),
            hidden,
            bytes,
        };
        let page = snapshot.page(0, page_size)?;
        state.entries.push_back(snapshot);
        Ok(page)
    }

    pub(super) fn page(
        &self,
        cursor: &PageCursor,
        page_size: Option<TaskPageSize>,
    ) -> Result<TaskListPage, ListTasksError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| ListTasksError::SnapshotUnavailable)?;
        state.prune(Instant::now());
        let snapshot = state
            .entries
            .iter()
            .find(|entry| Some(&entry.id) == cursor.snapshot.as_ref())
            .ok_or(ListTasksError::InvalidPageToken {
                reason: "snapshot expired or evicted; restart the listing",
            })?;
        if snapshot.binding != cursor.binding {
            return Err(ListTasksError::InvalidPageToken {
                reason: "snapshot query changed",
            });
        }
        let page_size = page_size.ok_or(ListTasksError::InvalidPageToken {
            reason: "snapshot requires a page size",
        })?;
        let start = snapshot
            .tasks
            .iter()
            .position(|task| task.id == cursor.after)
            .map(|index| index + 1)
            .ok_or(ListTasksError::InvalidPageToken {
                reason: "cursor task is not in the snapshot",
            })?;
        snapshot.page(start, page_size)
    }
}

impl SnapshotState {
    fn prune(&mut self, now: Instant) {
        self.entries
            .retain(|entry| now.duration_since(entry.created) < SNAPSHOT_LIFETIME);
    }
}

impl Snapshot {
    fn page(&self, start: usize, page_size: TaskPageSize) -> Result<TaskListPage, ListTasksError> {
        let end = start.saturating_add(page_size.get()).min(self.tasks.len());
        let token = if end < self.tasks.len() {
            Some(encode_page_token(
                &self.binding,
                &self.tasks[end - 1].id,
                Some(&self.id),
            )?)
        } else {
            None
        };
        Ok(TaskListPage {
            page: CursorPage {
                items: self.tasks[start..end].to_vec(),
                next_key: token,
            },
            hidden: self.hidden,
        })
    }
}

fn snapshot_bytes(tasks: &[ListedTask]) -> usize {
    size_of_val(tasks) + tasks.iter().map(task_bytes).sum::<usize>()
}

fn task_bytes(task: &ListedTask) -> usize {
    let fields = [
        task.id.as_ref(),
        task.project.as_ref(),
        task.heading.as_ref(),
        task.section.as_ref().map_or("", AsRef::as_ref),
        task.tags.as_ref().map_or("", AsRef::as_ref),
    ];
    let fields_bytes = fields.iter().map(|field| field.len()).sum::<usize>();
    fields_bytes + task.details.as_ref().map_or(0, details_bytes)
}

fn details_bytes(task: &pwf_wire::task::ListedTaskDetails) -> usize {
    let fields_bytes = task.prompt.as_ref().len()
        + task
            .project_path
            .as_ref()
            .map_or(0, |path| path.as_ref().len())
        + task.location.index_path().as_path().as_os_str().len();
    let blockers_bytes = task.blocked_by.as_ref().map_or(0, |blockers| {
        blockers
            .iter()
            .map(|id| size_of_val(id) + id.as_ref().len())
            .sum::<usize>()
    });
    let issues_bytes = task
        .blocked_by_issues
        .iter()
        .map(|issue| match issue {
            BlockedByIssue::Malformed { path, raw, reason } => {
                size_of_val(issue) + path.as_path().as_os_str().len() + raw.len() + reason.len()
            }
        })
        .sum::<usize>();
    let launch_bytes = task
        .launch
        .issues()
        .iter()
        .map(|issue| {
            size_of::<TaskIssue>()
                + match issue {
                    TaskIssue::MissingNote { path } => path.as_path().as_os_str().len(),
                    TaskIssue::PlaceholderPrompt => 0,
                }
        })
        .sum::<usize>();
    fields_bytes + blockers_bytes + issues_bytes + launch_bytes
}

#[cfg(test)]
mod tests {
    use pwf_wire::task::RawTaskTags;

    use super::*;
    use crate::{ports::task_vault::TaskSummaryRecord, task::task_view, testing::task_record};

    fn tasks() -> Vec<ListedTask> {
        ["FOO-0001", "FOO-0002"]
            .into_iter()
            .map(|id| {
                task_view::summarize(
                    TaskSummaryRecord::from(task_record(id)),
                    pwf_models::project::ProjectName::try_new("foo").unwrap(),
                )
                .unwrap()
            })
            .collect()
    }

    fn first_cursor(snapshots: &ListTasksSnapshots, tasks: Vec<ListedTask>) -> PageCursor {
        let result = snapshots
            .first_page(tasks, 7, Some(TaskPageSize::try_new(1).unwrap()), "query")
            .unwrap();
        super::super::decode_page_token(&result.page.next_key.unwrap()).unwrap()
    }

    #[test]
    fn snapshots_replay_pages_and_preserve_hidden_count() {
        let snapshots = ListTasksSnapshots::default();
        let cursor = first_cursor(&snapshots, tasks());
        let size = Some(TaskPageSize::try_new(1).unwrap());
        let first = snapshots.page(&cursor, size).unwrap();
        assert_eq!(first.page.items[0].id.as_ref(), "FOO-0002");
        assert_eq!(first.hidden, 7);
        assert_eq!(first.page.next_key, None);
        assert_eq!(snapshots.page(&cursor, size).unwrap(), first);
    }

    #[test]
    fn expired_and_evicted_tokens_require_a_fresh_listing() {
        let snapshots = ListTasksSnapshots::default();
        let expired = first_cursor(&snapshots, tasks());
        snapshots.state.lock().unwrap().entries[0].created -= SNAPSHOT_LIFETIME;
        let size = Some(TaskPageSize::try_new(1).unwrap());
        assert!(matches!(
            snapshots.page(&expired, size),
            Err(ListTasksError::InvalidPageToken { .. })
        ));
        let evicted = first_cursor(&snapshots, tasks());
        for _ in 0..SNAPSHOT_COUNT_MAX {
            first_cursor(&snapshots, tasks());
        }
        assert_eq!(
            snapshots.state.lock().unwrap().entries.len(),
            SNAPSHOT_COUNT_MAX
        );
        assert!(matches!(
            snapshots.page(&evicted, size),
            Err(ListTasksError::InvalidPageToken { .. })
        ));
    }

    #[test]
    fn snapshot_payload_budget_evicts_and_falls_back_for_large_lists() {
        let snapshots = ListTasksSnapshots::default();
        let mut large = tasks();
        large[0].tags = Some(RawTaskTags::new("x".repeat(SNAPSHOT_BYTES_MAX / 2)));
        let evicted = first_cursor(&snapshots, large.clone());
        first_cursor(&snapshots, large.clone());
        assert_eq!(snapshots.state.lock().unwrap().entries.len(), 1);
        assert!(
            snapshots
                .page(&evicted, Some(TaskPageSize::try_new(1).unwrap()))
                .is_err()
        );
        let tags = large[0].tags.clone();
        large[1].tags = tags;
        let cursor = first_cursor(&snapshots, large);
        assert_eq!(cursor.snapshot, None);
        assert!(
            snapshots
                .state
                .lock()
                .unwrap()
                .entries
                .iter()
                .map(|entry| entry.bytes)
                .sum::<usize>()
                <= SNAPSHOT_BYTES_MAX
        );
    }

    #[test]
    fn tokens_cannot_be_rebound_to_another_query_or_instance() {
        let snapshots = ListTasksSnapshots::default();
        let mut cursor = first_cursor(&snapshots, tasks());
        let size = Some(TaskPageSize::try_new(1).unwrap());
        assert!(ListTasksSnapshots::default().page(&cursor, size).is_err());
        cursor.binding = "different query".to_string();
        assert!(matches!(
            snapshots.page(&cursor, size),
            Err(ListTasksError::InvalidPageToken {
                reason: "snapshot query changed"
            })
        ));
    }
}
