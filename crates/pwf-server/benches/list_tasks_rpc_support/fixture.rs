use std::{
    collections::BTreeSet,
    fmt::Write as _,
    fs,
    num::NonZeroUsize,
    path::{Path, PathBuf},
};

use anyhow::{Context as _, ensure};
use pwf_models::{
    project::{ProjectId, ProjectName},
    task::{TaskId, TaskStatus, order::OrderSpec},
};
use pwf_wire::task::{TaskListLimit, TaskPageSize};
use serde::Deserialize;

const FIXTURE_MANIFEST_SOURCE: &str = include_str!("../fixtures/list-tasks-rpc.toml");
const FIXTURE_SCHEMA_VERSION: u32 = 1;
const PROJECT_COUNT_MAX: usize = 26 * 26;
const TASK_BODY_LINE_COUNT_MAX: usize = 4_096;
const TASK_COUNT_PER_PROJECT_MAX: usize = 9_999;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum WorkloadOperation {
    SingleRpc,
    AllPages,
}

#[derive(Debug, Deserialize)]
struct RawFixtureManifest {
    schema_version: u32,
    task_body_line_count: usize,
    workloads: Vec<RawWorkloadSpec>,
}

#[derive(Debug, Deserialize)]
struct RawWorkloadSpec {
    name: String,
    operation: WorkloadOperation,
    project_count: usize,
    task_count_per_project: usize,
    order: Option<String>,
}

pub struct FixtureManifest {
    schema_version: u32,
    task_body_line_count: NonZeroUsize,
    workloads: Vec<WorkloadSpec>,
}

impl FixtureManifest {
    pub fn parse() -> anyhow::Result<Self> {
        let raw: RawFixtureManifest =
            toml::from_str(FIXTURE_MANIFEST_SOURCE).context("parse fixture manifest")?;
        ensure!(
            raw.schema_version == FIXTURE_SCHEMA_VERSION,
            "fixture schema version {} is unsupported",
            raw.schema_version
        );
        let task_body_line_count = nonzero_bounded(
            raw.task_body_line_count,
            TASK_BODY_LINE_COUNT_MAX,
            "task_body_line_count",
        )?;
        ensure!(
            !raw.workloads.is_empty(),
            "fixture workloads must not be empty"
        );

        let mut workload_names = BTreeSet::new();
        let mut workloads = Vec::with_capacity(raw.workloads.len());
        for raw_workload in raw.workloads {
            let workload = WorkloadSpec::try_from(raw_workload)?;
            ensure!(
                workload_names.insert(workload.name.clone()),
                "fixture workload name {:?} is duplicated",
                workload.name
            );
            workloads.push(workload);
        }

        Ok(Self {
            schema_version: raw.schema_version,
            task_body_line_count,
            workloads,
        })
    }

    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    pub const fn task_body_line_count(&self) -> usize {
        self.task_body_line_count.get()
    }

    pub fn workloads(&self) -> &[WorkloadSpec] {
        &self.workloads
    }
}

pub struct WorkloadSpec {
    name: String,
    project_count: NonZeroUsize,
    task_count_per_project: NonZeroUsize,
    task_count_total: usize,
    order: Option<OrderSpec>,
}

impl WorkloadSpec {
    pub const fn order(&self) -> Option<OrderSpec> {
        self.order
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub const fn project_count(&self) -> usize {
        self.project_count.get()
    }

    pub const fn task_count_per_project(&self) -> usize {
        self.task_count_per_project.get()
    }

    pub const fn task_count_total(&self) -> usize {
        self.task_count_total
    }

    pub const fn page_count_expected(&self) -> usize {
        self.task_count_total.div_ceil(TaskPageSize::MAX)
    }
}

impl TryFrom<RawWorkloadSpec> for WorkloadSpec {
    type Error = anyhow::Error;

    fn try_from(raw: RawWorkloadSpec) -> Result<Self, Self::Error> {
        ensure!(
            valid_workload_name(&raw.name),
            "fixture workload name {:?} must contain only lowercase ASCII letters, digits, and hyphens",
            raw.name
        );
        let project_count = nonzero_bounded(raw.project_count, PROJECT_COUNT_MAX, "project_count")?;
        let task_count_per_project = nonzero_bounded(
            raw.task_count_per_project,
            TASK_COUNT_PER_PROJECT_MAX,
            "task_count_per_project",
        )?;
        let task_count_total = project_count
            .get()
            .checked_mul(task_count_per_project.get())
            .context("fixture task count overflowed")?;
        ensure!(
            task_count_total <= TaskListLimit::MAX,
            "fixture task count {task_count_total} exceeds {}",
            TaskListLimit::MAX
        );
        match raw.operation {
            WorkloadOperation::SingleRpc => ensure!(
                task_count_total == TaskPageSize::MAX,
                "single-rpc workload must contain exactly {} tasks",
                TaskPageSize::MAX
            ),
            WorkloadOperation::AllPages => ensure!(
                task_count_total > TaskPageSize::MAX,
                "all-pages workload must contain more than {} tasks",
                TaskPageSize::MAX
            ),
        }

        Ok(Self {
            order: raw.order.map(|order| order.parse()).transpose()?,
            name: raw.name,
            project_count,
            task_count_per_project,
            task_count_total,
        })
    }
}

pub struct PreparedFixture {
    pub projects: Vec<PreparedProject>,
    pub task_ids_expected: BTreeSet<String>,
}

pub struct PreparedProject {
    pub id: ProjectId,
    pub title: ProjectName,
    pub source_path: PathBuf,
    pub tasks_path: PathBuf,
}

pub fn prepare(
    root: &Path,
    workload: &WorkloadSpec,
    task_body_line_count: usize,
) -> anyhow::Result<PreparedFixture> {
    let vault_path = root.join("vault");
    fs::create_dir_all(vault_path.join(".obsidian"))?;
    let mut projects = Vec::with_capacity(workload.project_count());
    let mut task_ids_expected = BTreeSet::new();

    for project_index in 0..workload.project_count() {
        let project = prepare_project(
            root,
            &vault_path,
            project_index,
            workload,
            task_body_line_count,
            &mut task_ids_expected,
        )?;
        projects.push(project);
    }
    ensure!(
        task_ids_expected.len() == workload.task_count_total(),
        "fixture produced {} task IDs instead of {}",
        task_ids_expected.len(),
        workload.task_count_total()
    );

    Ok(PreparedFixture {
        projects,
        task_ids_expected,
    })
}

fn prepare_project(
    root: &Path,
    vault_path: &Path,
    project_index: usize,
    workload: &WorkloadSpec,
    task_body_line_count: usize,
    task_ids_expected: &mut BTreeSet<String>,
) -> anyhow::Result<PreparedProject> {
    let task_count = workload.task_count_per_project();
    let id = project_id(project_index)?;
    let title = ProjectName::try_new(format!("benchmark-project-{project_index:02}"))?;
    let source_path = root.join("projects").join(title.as_ref());
    let tasks_path = vault_path.join(title.as_ref());
    fs::create_dir_all(&source_path)?;
    fs::create_dir_all(&tasks_path)?;

    for task_number in 1..=task_count {
        let task_id = TaskId::try_new(format!("{id}-{task_number:04}"))?;
        let status = task_status(task_number);
        let mut source = task_source(&task_id, &title, task_number, status, task_body_line_count);
        if workload.order().is_some() {
            source = mixed_task_source(&source, task_number, task_count);
        }
        fs::write(tasks_path.join(format!("{task_id}.md")), source)?;
        ensure!(
            task_ids_expected.insert(task_id.into_string()),
            "fixture generated a duplicate task ID"
        );
    }

    Ok(PreparedProject {
        id,
        title,
        source_path,
        tasks_path,
    })
}

fn task_source(
    task_id: &TaskId,
    project: &ProjectName,
    task_number: usize,
    status: TaskStatus,
    body_line_count: usize,
) -> String {
    let mut source = format!(
        concat!(
            "---\n",
            "id: {task_id}\n",
            "status: {status}\n",
            "title: benchmark task {task_number:04}\n",
            "project: {project}\n",
            "created_at: 2026-09-04T12:34:56Z\n",
            "effort: medium\n",
            "priority: high\n",
            "tags: [\"benchmark\", \"transport\"]\n",
            "---\n\n"
        ),
        task_id = task_id,
        status = status,
        task_number = task_number,
        project = project,
    );
    for line_index in 0..body_line_count {
        let _ = writeln!(
            source,
            "Benchmark body line {line_index:04} for synthetic task {task_id}."
        );
    }
    source
}

const fn task_status(task_number: usize) -> TaskStatus {
    match task_number % 3 {
        1 => TaskStatus::Active,
        2 => TaskStatus::Done,
        _ => TaskStatus::Cancelled,
    }
}

fn project_id(project_index: usize) -> anyhow::Result<ProjectId> {
    let first_suffix = u8::try_from(project_index / 26)?;
    let second_suffix = u8::try_from(project_index % 26)?;
    let first = char::from(b'A' + first_suffix);
    let second = char::from(b'A' + second_suffix);
    ProjectId::try_new(format!("B{first}{second}")).map_err(Into::into)
}

fn nonzero_bounded(value: usize, maximum: usize, field: &str) -> anyhow::Result<NonZeroUsize> {
    ensure!(
        value <= maximum,
        "fixture {field} {value} exceeds {maximum}"
    );
    NonZeroUsize::new(value).with_context(|| format!("fixture {field} must be nonzero"))
}

fn valid_workload_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn mixed_task_source(source: &str, task_number: usize, task_count: usize) -> String {
    let priority = match task_number % 5 {
        0 => "",
        1 => "priority: low\n",
        2 => "priority: medium\n",
        3 => "priority: high\n",
        _ => "priority: highest\n",
    };
    let effort = match (task_number / 5) % 5 {
        0 => "",
        1 => "effort: low\n",
        2 => "effort: medium\n",
        3 => "effort: high\n",
        _ => "effort: highest\n",
    };
    source
        .replace("priority: high\n", priority)
        .replace("effort: medium\n", effort)
        .replace(
            &format!("title: benchmark task {task_number:04}"),
            &format!("title: varied task {:04}", task_number * 17 % task_count),
        )
}
