use pwf_wire::project::ProjectStatusFilter;

use crate::{
    ports::task_metadata_migration::{
        TaskMetadataMigrationClient, TaskMetadataMigrationIssue, TaskMetadataMigrationMode,
        TaskMetadataMigrationReport,
    },
    project::list_projects,
};

#[derive(Debug, thiserror::Error)]
pub enum MigrateTaskMetadataError {
    #[error("cannot list managed projects: {0}")]
    ListProjects(#[source] anyhow::Error),
}

/// Migrates task metadata in every active and paused managed project.
#[cqrsy::command]
pub async fn execute(
    mode: TaskMetadataMigrationMode,
    client: &impl TaskMetadataMigrationClient,
    pool: &sqlx::SqlitePool,
) -> Result<TaskMetadataMigrationReport, MigrateTaskMetadataError> {
    let projects = list_projects::execute(ProjectStatusFilter::IncludingPaused, pool)
        .await
        .map_err(|source| MigrateTaskMetadataError::ListProjects(anyhow::Error::new(source)))?;
    let mut report = TaskMetadataMigrationReport::default();
    for project in projects {
        report.project_count += 1;
        match client.migrate_project_task_metadata(&project, mode) {
            Ok(project_report) => report.include(project_report),
            Err(source) => report.issues.push(TaskMetadataMigrationIssue::new(
                project.title,
                None,
                source.to_string(),
            )),
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use pwf_models::project::Project;

    use crate::{
        ports::task_metadata_migration::{
            TaskMetadataMigrationClient, TaskMetadataMigrationMode, TaskMetadataMigrationReport,
        },
        task::migrate_task_metadata,
        testing::insert_project,
    };

    #[derive(Debug, Clone, Copy, thiserror::Error)]
    #[error("injected migration failure")]
    struct InjectedError;

    #[derive(Clone, Default)]
    struct RecordingClient {
        projects: Arc<Mutex<Vec<String>>>,
    }

    impl TaskMetadataMigrationClient for RecordingClient {
        type Error = InjectedError;

        fn migrate_project_task_metadata(
            &self,
            project: &Project,
            mode: TaskMetadataMigrationMode,
        ) -> Result<TaskMetadataMigrationReport, Self::Error> {
            assert_eq!(mode, TaskMetadataMigrationMode::Check);
            self.projects
                .lock()
                .unwrap()
                .push(project.title.to_string());
            project_report(project)
        }
    }

    fn project_report(project: &Project) -> Result<TaskMetadataMigrationReport, InjectedError> {
        match project.title.as_ref() {
            "beta" => Err(InjectedError),
            _ => Ok(TaskMetadataMigrationReport {
                task_file_count: 1,
                task_file_changed_count: 1,
                created_at_migrated_count: 1,
                ..TaskMetadataMigrationReport::default()
            }),
        }
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn migration_visits_paused_projects_and_continues_after_a_failure(
        pool: sqlx::SqlitePool,
    ) {
        insert_project(&pool, "ALP", "alpha", "/work/alpha", "/tasks/alpha", false).await;
        insert_project(&pool, "BET", "beta", "/work/beta", "/tasks/beta", true).await;
        insert_project(&pool, "GAM", "gamma", "/work/gamma", "/tasks/gamma", false).await;
        let client = RecordingClient::default();

        let report =
            migrate_task_metadata::execute(TaskMetadataMigrationMode::Check, &client, &pool)
                .await
                .unwrap();

        assert_eq!(*client.projects.lock().unwrap(), ["alpha", "beta", "gamma"]);
        assert_eq!(report.project_count, 3);
        assert_eq!(report.task_file_count, 2);
        assert_eq!(report.task_file_changed_count, 2);
        assert_eq!(report.created_at_migrated_count, 2);
        assert_eq!(report.issues.len(), 1);
        assert_eq!(report.issues[0].project.as_ref(), "beta");
        assert_eq!(report.issues[0].message, "injected migration failure");
    }
}
