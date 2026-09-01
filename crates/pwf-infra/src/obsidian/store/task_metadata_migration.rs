use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use lazy_regex::{Regex, regex};
use pwf_application::ports::{
    task_metadata_migration::{
        TaskMetadataMigrationClient, TaskMetadataMigrationIssue, TaskMetadataMigrationMode,
        TaskMetadataMigrationReport,
    },
    task_vault::IndexEntryState,
};
use pwf_models::{
    AppDate,
    project::Project,
    task::{TaskId, TaskStatus, TaskTimestamp},
};

use super::{
    ObsidianStore, ObsidianStoreError,
    fs::write_index,
    index_entry::{ParsedIndexLine, parse_index_lines},
};
use crate::obsidian::{
    MarkdownFile,
    identity::{
        parse_project_index_identity, parse_task_metadata_if_task, validate_project_index_identity,
    },
};

fn completion_stamp_regex() -> &'static Regex {
    regex!(r"[ \t]*✅[ \t]*\d{4}-\d{2}-\d{2}")
}

struct ProjectIndexMigration {
    path: PathBuf,
    source: String,
    parsed_lines: Vec<ParsedIndexLine>,
    completed_at_by_id: BTreeMap<TaskId, TaskTimestamp>,
}

#[derive(Default)]
struct TaskFileMigration {
    changed: bool,
    created_at_migrated_count: usize,
    completed_at_migrated_count: usize,
    completion: CompletionMigration,
    issues: Vec<String>,
}

#[derive(Default)]
enum CompletionMigration {
    #[default]
    Absent,
    Present,
    PresentFromModified,
}

impl CompletionMigration {
    const fn is_present(&self) -> bool {
        matches!(self, Self::Present | Self::PresentFromModified)
    }

    const fn used_modified_time(&self) -> bool {
        matches!(self, Self::PresentFromModified)
    }
}

#[derive(Default)]
struct TaskFields {
    status: Option<String>,
    created: Option<String>,
    created_at: Option<String>,
    completed: Option<String>,
    completed_at: Option<String>,
}

struct TaskMigrationContext<'a> {
    project: &'a Project,
    index_completed_at: &'a BTreeMap<TaskId, TaskTimestamp>,
    mode: TaskMetadataMigrationMode,
}

impl TaskMetadataMigrationClient for ObsidianStore {
    type Error = ObsidianStoreError;

    fn migrate_project_task_metadata(
        &self,
        project: &Project,
        mode: TaskMetadataMigrationMode,
    ) -> Result<TaskMetadataMigrationReport, Self::Error> {
        self.migrate_project_metadata(project, mode)
    }
}

impl ObsidianStore {
    fn migrate_project_metadata(
        &self,
        project: &Project,
        mode: TaskMetadataMigrationMode,
    ) -> Result<TaskMetadataMigrationReport, ObsidianStoreError> {
        let tasks_path = self.tasks_path(project)?;
        let index_path = self.project_index_path(project)?;
        let mut report = TaskMetadataMigrationReport::default();
        let index = self.inspect_migration_index(project, &mut report);
        let index_completed_at = index
            .as_ref()
            .map_or_else(BTreeMap::new, |index| index.completed_at_by_id.clone());
        let (paths, entry_errors) = task_markdown_paths(&tasks_path, &index_path)?;
        report.issues.extend(entry_errors.into_iter().map(|source| {
            TaskMetadataMigrationIssue::new(
                project.title.clone(),
                Some(tasks_path.clone()),
                format!("cannot inspect task directory entry: {source}"),
            )
        }));

        let mut task_ids_with_completion = BTreeSet::new();
        let context = TaskMigrationContext {
            project,
            index_completed_at: &index_completed_at,
            mode,
        };
        for path in paths {
            migrate_task_path(&path, &context, &mut report, &mut task_ids_with_completion);
        }
        if let Some(index) = index {
            migrate_index_stamps(
                project,
                &index,
                &task_ids_with_completion,
                mode,
                &mut report,
            );
        }
        Ok(report)
    }

    fn inspect_migration_index(
        &self,
        project: &Project,
        report: &mut TaskMetadataMigrationReport,
    ) -> Option<ProjectIndexMigration> {
        let path = match self.project_index_path(project) {
            Ok(path) => path,
            Err(error) => {
                report.issues.push(TaskMetadataMigrationIssue::new(
                    project.title.clone(),
                    None,
                    error.to_string(),
                ));
                return None;
            }
        };
        match path.try_exists() {
            Ok(false) => return None,
            Ok(true) => {}
            Err(error) => {
                push_issue(report, project, &path, error);
                return None;
            }
        }
        let file = match MarkdownFile::open(&path) {
            Ok(file) => file,
            Err(error) => {
                push_issue(report, project, &path, error);
                return None;
            }
        };
        let actual = match parse_project_index_identity(&file) {
            Ok(identity) => identity,
            Err(error) => {
                push_issue(report, project, &path, error);
                return None;
            }
        };
        if let Err(error) =
            validate_project_index_identity(&path, &actual, &Self::project_identity(project))
        {
            push_issue(report, project, &path, error);
            return None;
        }
        let source = file.into_source();
        let parsed_lines = match parse_index_lines(&path, &source) {
            Ok(lines) => lines,
            Err(error) => {
                report.issues.push(TaskMetadataMigrationIssue::new(
                    project.title.clone(),
                    Some(path),
                    error.to_string(),
                ));
                return None;
            }
        };
        let completed_at_by_id = parsed_lines
            .iter()
            .filter_map(|line| match line.state {
                IndexEntryState::Done(Some(completed_at)) => Some((line.id.clone(), completed_at)),
                IndexEntryState::Open | IndexEntryState::Done(None) => None,
            })
            .collect();
        Some(ProjectIndexMigration {
            path,
            source,
            parsed_lines,
            completed_at_by_id,
        })
    }
}

fn task_markdown_path(
    entry: Result<std::fs::DirEntry, std::io::Error>,
    index_path: &Path,
) -> Result<Option<PathBuf>, std::io::Error> {
    let path = entry?.path();
    Ok((path != index_path
        && path.extension().and_then(|extension| extension.to_str()) == Some("md"))
    .then_some(path))
}

fn task_markdown_paths(
    tasks_path: &Path,
    index_path: &Path,
) -> Result<(Vec<PathBuf>, Vec<std::io::Error>), ObsidianStoreError> {
    let inspected = std::fs::read_dir(tasks_path)
        .map_err(|source| ObsidianStoreError::ReadTaskFile { source })?
        .map(|entry| task_markdown_path(entry, index_path))
        .collect::<Vec<_>>();
    let mut paths = inspected
        .iter()
        .filter_map(|result| result.as_ref().ok().and_then(Option::as_ref).cloned())
        .collect::<Vec<_>>();
    paths.sort();
    let errors = inspected.into_iter().filter_map(Result::err).collect();
    Ok((paths, errors))
}

fn migrate_task_path(
    path: &Path,
    context: &TaskMigrationContext<'_>,
    report: &mut TaskMetadataMigrationReport,
    task_ids_with_completion: &mut BTreeSet<TaskId>,
) {
    let mut file = match MarkdownFile::open(path) {
        Ok(file) => file,
        Err(error) => {
            push_issue(report, context.project, path, error);
            return;
        }
    };
    let id = match parse_task_metadata_if_task(&file) {
        Ok(Some((id, _))) => id,
        Ok(None) => return,
        Err(error) => {
            push_issue(report, context.project, path, error);
            return;
        }
    };
    report.task_file_count += 1;
    let migration = migrate_task_file(&mut file, context.index_completed_at.get(&id));
    let migration = match migration {
        Ok(migration) => migration,
        Err(error) => {
            push_issue(report, context.project, path, error);
            return;
        }
    };
    for message in &migration.issues {
        report.issues.push(TaskMetadataMigrationIssue::new(
            context.project.title.clone(),
            Some(path.to_path_buf()),
            message,
        ));
    }
    if migration.changed
        && context.mode == TaskMetadataMigrationMode::Apply
        && let Err(error) = file.save()
    {
        push_issue(report, context.project, path, error);
        return;
    }
    if migration.changed {
        report.task_file_changed_count += 1;
    }
    report.created_at_migrated_count += migration.created_at_migrated_count;
    report.completed_at_migrated_count += migration.completed_at_migrated_count;
    report.completed_at_from_modified_count +=
        usize::from(migration.completion.used_modified_time());
    if migration.completion.is_present() {
        task_ids_with_completion.insert(id);
    }
}

fn migrate_task_file(
    file: &mut MarkdownFile,
    index_completed_at: Option<&TaskTimestamp>,
) -> anyhow::Result<TaskFileMigration> {
    let original = file.source().to_string();
    let fields = task_fields(file)?;
    let mut migration = TaskFileMigration::default();

    migrate_created_at(file, &fields, &mut migration)?;
    migrate_completed_at(file, &fields, index_completed_at, &mut migration)?;
    migration.changed = file.source() != original;
    Ok(migration)
}

fn task_fields(file: &MarkdownFile) -> anyhow::Result<TaskFields> {
    let frontmatter = file
        .frontmatter_view()?
        .ok_or_else(|| anyhow::anyhow!("task note has no frontmatter"))?;
    let get = |name| frontmatter.get(name).map(|value| value.map(str::to_string));
    Ok(TaskFields {
        status: get("status")?,
        created: get("created")?,
        created_at: get("created_at")?,
        completed: get("completed")?,
        completed_at: get("completed_at")?,
    })
}

fn migrate_created_at(
    file: &mut MarkdownFile,
    fields: &TaskFields,
    migration: &mut TaskFileMigration,
) -> anyhow::Result<()> {
    let canonical = parse_timestamp("created_at", fields.created_at.as_deref(), migration);
    if canonical.is_err() {
        return Ok(());
    }
    let canonical = canonical.ok().flatten();
    let mut next = canonical;
    if next.is_none() {
        match parse_date("created", fields.created.as_deref(), migration) {
            Ok(created) => next = created,
            Err(()) => return Ok(()),
        }
    }
    if canonical.is_none()
        && let Some(created_at) = next
    {
        file.set_property_rendered("created_at", Some(&created_at.to_string()), &["created"])?;
    }
    if fields.created.is_some() {
        file.remove_property("created")?;
        migration.created_at_migrated_count = 1;
    }
    Ok(())
}

fn migrate_completed_at(
    file: &mut MarkdownFile,
    fields: &TaskFields,
    index_completed_at: Option<&TaskTimestamp>,
    migration: &mut TaskFileMigration,
) -> anyhow::Result<()> {
    let canonical = parse_timestamp("completed_at", fields.completed_at.as_deref(), migration);
    if canonical.is_err() {
        return Ok(());
    }
    let canonical = canonical.ok().flatten();
    let mut next = canonical;
    let mut from_modified = false;
    if next.is_none() {
        match parse_date("completed", fields.completed.as_deref(), migration) {
            Ok(completed) => next = completed,
            Err(()) => return Ok(()),
        }
    }
    if next.is_none() {
        next = index_completed_at.copied();
    }
    if next.is_none() && is_closed(fields.status.as_deref(), migration) {
        match file_modified_at(file.path()) {
            Ok(completed_at) => {
                next = Some(completed_at);
                from_modified = true;
            }
            Err(error) => migration
                .issues
                .push(format!("cannot read file modification timestamp: {error}")),
        }
    }
    if canonical.is_none()
        && let Some(completed_at) = next
    {
        file.set_property_rendered(
            "completed_at",
            Some(&completed_at.to_string()),
            &["completed", "created_at", "created"],
        )?;
        migration.completed_at_migrated_count = 1;
    }
    if fields.completed.is_some() {
        file.remove_property("completed")?;
        migration.completed_at_migrated_count = 1;
    }
    migration.completion = match (next.is_some(), from_modified) {
        (true, true) => CompletionMigration::PresentFromModified,
        (true, false) => CompletionMigration::Present,
        (false, _) => CompletionMigration::Absent,
    };
    Ok(())
}

fn parse_timestamp(
    property: &'static str,
    raw: Option<&str>,
    migration: &mut TaskFileMigration,
) -> Result<Option<TaskTimestamp>, ()> {
    let Some(raw) = raw.filter(|raw| !raw.trim().is_empty()) else {
        return Ok(None);
    };
    raw.parse::<TaskTimestamp>().map(Some).map_err(|error| {
        migration
            .issues
            .push(format!("invalid `{property}` value {raw:?}: {error}"));
    })
}

fn parse_date(
    property: &'static str,
    raw: Option<&str>,
    migration: &mut TaskFileMigration,
) -> Result<Option<TaskTimestamp>, ()> {
    let Some(raw) = raw.filter(|raw| !raw.trim().is_empty()) else {
        return Ok(None);
    };
    raw.parse::<AppDate>()
        .map_err(|error| {
            migration
                .issues
                .push(format!("invalid `{property}` value {raw:?}: {error}"));
        })
        .and_then(|date| {
            TaskTimestamp::at_midnight_utc(date)
                .map(Some)
                .map_err(|error| {
                    migration.issues.push(format!(
                        "cannot convert `{property}` value {raw:?}: {error}"
                    ));
                })
        })
}

fn is_closed(raw: Option<&str>, migration: &mut TaskFileMigration) -> bool {
    let Some(raw) = raw.filter(|raw| !raw.trim().is_empty()) else {
        return false;
    };
    match raw.parse::<TaskStatus>() {
        Ok(TaskStatus::Done | TaskStatus::Cancelled) => true,
        Ok(TaskStatus::Active) => false,
        Err(error) => {
            migration
                .issues
                .push(format!("invalid `status` value {raw:?}: {error}"));
            false
        }
    }
}

fn file_modified_at(path: &Path) -> anyhow::Result<TaskTimestamp> {
    let modified = std::fs::metadata(path)?.modified()?;
    let timestamp = jiff::Timestamp::try_from(modified)?;
    Ok(TaskTimestamp::from_timestamp(timestamp)?)
}

fn migrate_index_stamps(
    project: &Project,
    index: &ProjectIndexMigration,
    task_ids_with_completion: &BTreeSet<TaskId>,
    mode: TaskMetadataMigrationMode,
    report: &mut TaskMetadataMigrationReport,
) {
    let line_numbers = index
        .parsed_lines
        .iter()
        .filter(|line| {
            matches!(line.state, IndexEntryState::Done(Some(_)))
                && task_ids_with_completion.contains(&line.id)
        })
        .map(|line| line.line_number.get())
        .collect::<BTreeSet<_>>();
    let (updated, changed_count) = remove_index_completion_stamps(&index.source, &line_numbers);
    if changed_count == 0 {
        return;
    }
    if mode == TaskMetadataMigrationMode::Apply
        && let Err(error) = write_index(&index.path, &updated)
    {
        push_issue(report, project, &index.path, error);
        return;
    }
    report.index_entry_changed_count += changed_count;
}

fn remove_index_completion_stamps(source: &str, line_numbers: &BTreeSet<usize>) -> (String, usize) {
    let mut changed_count = 0;
    let updated = source
        .split_inclusive('\n')
        .enumerate()
        .map(|(index, line)| {
            if !line_numbers.contains(&(index + 1)) {
                return line.to_string();
            }
            let updated = completion_stamp_regex().replace(line, "").to_string();
            changed_count += usize::from(updated != line);
            updated
        })
        .collect();
    (updated, changed_count)
}

fn push_issue(
    report: &mut TaskMetadataMigrationReport,
    project: &Project,
    path: &Path,
    error: impl std::fmt::Display,
) {
    report.issues.push(TaskMetadataMigrationIssue::new(
        project.title.clone(),
        Some(path.to_path_buf()),
        error.to_string(),
    ));
}

#[cfg(test)]
mod tests {
    use pwf_application::ports::task_metadata_migration::{
        TaskMetadataMigrationClient, TaskMetadataMigrationMode,
    };
    use pwf_models::{
        project::{
            HomeDirectory, Project, ProjectSource, ProjectSourceKind, ProjectSourceValue,
            ProjectTasks, ProjectTasksKind, ProjectTasksPath,
        },
        task::TaskTimestamp,
    };

    use super::ObsidianStore;

    #[test]
    fn check_previews_and_apply_migrates_task_metadata_idempotently() -> anyhow::Result<()> {
        let directory = tempfile::tempdir()?;
        let tasks_path = directory.path().join("tasks");
        std::fs::create_dir(&tasks_path)?;
        let project = project(&tasks_path);
        let index_path = tasks_path.join("foo.md");
        let index = "---\r\nid: FOO\r\ntitle: foo\r\n---\r\n\r\n## Next\r\n- [ ] [[FOO-0001]]\r\n- [x] [[FOO-0002|done alias]] ✅ 2026-07-03\r\n- [x] [[FOO-0003]]\r\n- [x] [[FOO-0004]] ✅ 2026-07-04\r\n";
        std::fs::write(&index_path, index)?;
        let active_path = write_task(
            &tasks_path,
            "FOO-0001",
            "status: active\ncreated: 2026-07-01\n",
        )?;
        let done_path = write_task(
            &tasks_path,
            "FOO-0002",
            "status: done\ncreated: 2026-07-01\ncompleted: 2026-07-02\n",
        )?;
        let cancelled_path = write_task(
            &tasks_path,
            "FOO-0003",
            "status: cancelled\ncreated: 2026-07-01\n",
        )?;
        let sparse_path = write_task(&tasks_path, "FOO-0004", "status: active\n")?;
        std::fs::write(
            tasks_path.join("project note.md"),
            "---\nid: FOO-0099\ntype: note\ncreated: 2026-07-01\n---\n\nkeep\n",
        )?;
        let cancelled_modified_at = modified_at(&cancelled_path)?;
        let store = ObsidianStore::new(HomeDirectory::new(directory.path().to_path_buf()));

        let preview =
            store.migrate_project_task_metadata(&project, TaskMetadataMigrationMode::Check)?;

        assert_eq!(preview.task_file_count, 4);
        assert_eq!(preview.task_file_changed_count, 4);
        assert_eq!(preview.created_at_migrated_count, 3);
        assert_eq!(preview.completed_at_migrated_count, 3);
        assert_eq!(preview.completed_at_from_modified_count, 1);
        assert_eq!(preview.index_entry_changed_count, 2);
        assert!(preview.issues.is_empty());
        assert!(std::fs::read_to_string(&active_path)?.contains("created: 2026-07-01"));
        assert_eq!(std::fs::read_to_string(&index_path)?, index);

        let applied =
            store.migrate_project_task_metadata(&project, TaskMetadataMigrationMode::Apply)?;

        assert_eq!(applied.task_file_changed_count, 4);
        assert_eq!(
            std::fs::read_to_string(&active_path)?,
            task_source(
                "FOO-0001",
                "status: active\ncreated_at: 2026-07-01T00:00:00Z\n"
            )
        );
        assert_eq!(
            std::fs::read_to_string(&done_path)?,
            task_source(
                "FOO-0002",
                "status: done\ncreated_at: 2026-07-01T00:00:00Z\ncompleted_at: 2026-07-02T00:00:00Z\n"
            )
        );
        assert_eq!(
            std::fs::read_to_string(&cancelled_path)?,
            task_source(
                "FOO-0003",
                &format!(
                    "status: cancelled\ncreated_at: 2026-07-01T00:00:00Z\ncompleted_at: {cancelled_modified_at}\n"
                )
            )
        );
        assert_eq!(
            std::fs::read_to_string(&sparse_path)?,
            "---\nid: FOO-0004\nstatus: active\ntitle: task\ncompleted_at: 2026-07-04T00:00:00Z\n---\n\nbody\n"
        );
        assert_eq!(
            std::fs::read_to_string(&index_path)?,
            index
                .replace(" ✅ 2026-07-03", "")
                .replace(" ✅ 2026-07-04", "")
        );

        let current =
            store.migrate_project_task_metadata(&project, TaskMetadataMigrationMode::Check)?;
        assert!(!current.has_changes());
        assert!(current.issues.is_empty());
        Ok(())
    }

    #[test]
    fn invalid_canonical_timestamp_does_not_block_other_task_files() -> anyhow::Result<()> {
        let directory = tempfile::tempdir()?;
        let tasks_path = directory.path().join("tasks");
        std::fs::create_dir(&tasks_path)?;
        let project = project(&tasks_path);
        std::fs::write(tasks_path.join("foo.md"), "---\nid: FOO\ntitle: foo\n---\n")?;
        let invalid_path = write_task(
            &tasks_path,
            "FOO-0001",
            "status: active\ncreated: 2026-07-01\ncreated_at: yesterday\n",
        )?;
        let valid_path = write_task(
            &tasks_path,
            "FOO-0002",
            "status: active\ncreated: 2026-07-02\n",
        )?;
        let store = ObsidianStore::new(HomeDirectory::new(directory.path().to_path_buf()));

        let report =
            store.migrate_project_task_metadata(&project, TaskMetadataMigrationMode::Apply)?;

        assert_eq!(report.task_file_count, 2);
        assert_eq!(report.task_file_changed_count, 1);
        assert_eq!(report.issues.len(), 1);
        assert!(report.issues[0].message.contains("invalid `created_at`"));
        assert!(std::fs::read_to_string(&invalid_path)?.contains("created: 2026-07-01"));
        assert!(std::fs::read_to_string(&valid_path)?.contains("created_at: 2026-07-02T00:00:00Z"));
        Ok(())
    }

    fn project(tasks_path: &std::path::Path) -> Project {
        Project {
            id: "FOO".parse().unwrap(),
            title: pwf_models::project::ProjectName::try_new("foo").unwrap(),
            source: ProjectSource::new(
                ProjectSourceKind::Directory,
                ProjectSourceValue::try_new(tasks_path.display().to_string()).unwrap(),
            ),
            tasks: ProjectTasks::new(
                ProjectTasksKind::Directory,
                ProjectTasksPath::try_new(tasks_path.display().to_string()).unwrap(),
            ),
            created_at: "2026-07-01T00:00:00Z".parse().unwrap(),
            is_paused: false,
        }
    }

    fn write_task(
        tasks_path: &std::path::Path,
        id: &str,
        fields: &str,
    ) -> anyhow::Result<std::path::PathBuf> {
        let path = tasks_path.join(format!("{id}.md"));
        std::fs::write(&path, task_source(id, fields))?;
        Ok(path)
    }

    fn task_source(id: &str, fields: &str) -> String {
        format!("---\nid: {id}\n{fields}title: task\n---\n\nbody\n")
    }

    fn modified_at(path: &std::path::Path) -> anyhow::Result<TaskTimestamp> {
        let modified = std::fs::metadata(path)?.modified()?;
        let timestamp = jiff::Timestamp::try_from(modified)?;
        Ok(TaskTimestamp::from_timestamp(timestamp)?)
    }
}
