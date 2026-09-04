use std::{collections::BTreeSet, time::Duration};

use anyhow::{Context as _, ensure};
use pwf_client::{
    pb::{self, AllTaskSections, ListDetail, ListTasksRequest, list_tasks_request},
    task::TaskClient,
};
use pwf_wire::task::TaskPageSize;

use super::fixture::{FixtureManifest, PreparedFixture, WorkloadSpec, prepare};
use crate::server::TestServer;

const SERVER_SHUTDOWN_GRACE_PERIOD: Duration = Duration::from_secs(2);

pub struct ListTasksRpcWorkload {
    server: TestServer,
    task_client: TaskClient,
    request: ListTasksRequest,
    task_ids_expected: BTreeSet<String>,
    page_count_expected: usize,
}

pub struct ListTasksRpcMeasurement {
    tasks: Vec<pb::TaskView>,
    page_count: usize,
}

impl ListTasksRpcWorkload {
    pub async fn start(
        manifest: &FixtureManifest,
        workload: &WorkloadSpec,
    ) -> anyhow::Result<Self> {
        let server = TestServer::start(SERVER_SHUTDOWN_GRACE_PERIOD).await?;
        let prepared = prepare(
            server.root.path(),
            workload,
            manifest.task_body_line_count(),
        )?;
        register_projects(&server, &prepared).await?;
        let task_client = server.client.task();

        Ok(Self {
            server,
            task_client,
            request: list_tasks_request()?,
            task_ids_expected: prepared.task_ids_expected,
            page_count_expected: workload.page_count_expected(),
        })
    }

    pub async fn measure(&self) -> anyhow::Result<ListTasksRpcMeasurement> {
        let mut request = self.request.clone();
        let mut tasks = Vec::with_capacity(self.task_ids_expected.len());
        for page_count in 1..=self.page_count_expected {
            let mut response = self
                .task_client
                .list_tasks(request.clone())
                .await
                .context("request task-list page")?;
            tasks.append(&mut response.tasks);
            match response.next_page_token {
                None => return Ok(ListTasksRpcMeasurement { tasks, page_count }),
                Some(token) if page_count < self.page_count_expected => {
                    request.page_token = Some(token);
                }
                Some(_) => anyhow::bail!(
                    "task-list response exceeded the expected {} pages",
                    self.page_count_expected
                ),
            }
        }
        anyhow::bail!("task-list response did not produce a final page")
    }

    pub fn validate(&self, measurement: &ListTasksRpcMeasurement) -> anyhow::Result<()> {
        ensure!(
            measurement.page_count == self.page_count_expected,
            "task-list returned {} pages instead of {}",
            measurement.page_count,
            self.page_count_expected
        );
        ensure!(
            measurement.tasks.len() == self.task_ids_expected.len(),
            "task-list returned {} tasks instead of {}",
            measurement.tasks.len(),
            self.task_ids_expected.len()
        );
        let task_ids_actual = measurement
            .tasks
            .iter()
            .map(|task| task.id.clone())
            .collect::<BTreeSet<_>>();
        ensure!(
            task_ids_actual.len() == measurement.tasks.len(),
            "task-list returned duplicate task IDs"
        );
        ensure!(
            task_ids_actual == self.task_ids_expected,
            "task-list returned an unexpected task set"
        );
        Ok(())
    }

    pub async fn finish(self) -> anyhow::Result<()> {
        self.server.finish().await
    }
}

async fn register_projects(server: &TestServer, prepared: &PreparedFixture) -> anyhow::Result<()> {
    let client = server.client.project();
    for project in &prepared.projects {
        let response = client
            .add_project(pb::AddProjectRequest {
                fields: Some(pb::ProjectFields {
                    id: project.id.to_string(),
                    title: project.title.to_string(),
                    source_kind: "directory".to_string(),
                    source_value: project.source_path.to_string_lossy().into_owned(),
                    tasks_kind: "directory".to_string(),
                    tasks_path: project.tasks_path.to_string_lossy().into_owned(),
                }),
            })
            .await
            .with_context(|| format!("register benchmark project {}", project.id))?;
        ensure!(
            response.id == project.id.as_ref(),
            "registered project returned ID {:?} instead of {}",
            response.id,
            project.id
        );
    }
    Ok(())
}

fn list_tasks_request() -> anyhow::Result<ListTasksRequest> {
    Ok(ListTasksRequest {
        project_selector: None,
        scope: Some(list_tasks_request::Scope::All(AllTaskSections {})),
        number: None,
        effort: None,
        tags: Vec::new(),
        order: None,
        status: None,
        detail: ListDetail::Summary as i32,
        priority: None,
        page_size: u32::try_from(TaskPageSize::MAX).context("convert task-list page size")?,
        page_token: None,
    })
}
