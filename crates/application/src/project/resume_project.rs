use std::{error::Error, path::PathBuf};

use pwf_models::project::ProjectPrefix;

use super::{
    ProjectStateChange,
    dto::{ProjectRow, ProjectRowError},
    logic::task_location::{self, TaskLocationError},
};
use crate::AppDbStore;

/// Requests resuming one managed project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumeProject {
    /// Project prefix.
    pub id: ProjectPrefix,
    /// Home directory used to expand home-relative task paths.
    pub home: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum ResumeProjectError {
    #[error("project not found: {id}")]
    ProjectNotFound { id: ProjectPrefix },
    #[error("managed project {project_id} task path '{path}' is invalid: {source}")]
    InvalidTaskPath {
        project_id: ProjectPrefix,
        path: String,
        #[source]
        source: super::resolve_runtime_path::RuntimePathError,
    },
    #[error(
        "managed projects {first_id} and {second_id} resolve to the same task location: {}",
        path.display()
    )]
    DuplicateRuntimeTaskLocation {
        first_id: ProjectPrefix,
        second_id: ProjectPrefix,
        path: PathBuf,
    },
    #[error("{context}: {source}")]
    Unexpected {
        context: &'static str,
        #[source]
        source: Box<dyn Error + Send + Sync>,
    },
}

/// Resumes one project and reports whether persisted state changed.
///
/// # Errors
///
/// Returns [`ResumeProjectError::ProjectNotFound`] when no project has the requested ID. Returns
/// [`ResumeProjectError::InvalidTaskPath`] when the resumed or another project task path cannot be
/// resolved, and [`ResumeProjectError::DuplicateRuntimeTaskLocation`] when two projects resolve to
/// the same task location. Returns [`ResumeProjectError::Unexpected`] for database and
/// persisted-data failures.
#[cqrsy::command]
pub async fn execute(
    command: ResumeProject,
    database: &impl AppDbStore,
) -> Result<ProjectStateChange, ResumeProjectError> {
    let mut transaction = database
        .pool()
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(|error| unexpected("starting project resume transaction", error))?;
    let id = command.id.as_ref();
    let candidate_path =
        sqlx::query_scalar::<_, String>("SELECT tasks_path FROM projects WHERE id = ?")
            .bind(id)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(|error| unexpected("reading resumed project task location", error))?
            .ok_or_else(|| ResumeProjectError::ProjectNotFound {
                id: command.id.clone(),
            })?;
    let existing = sqlx::query_as::<_, (String, String)>(
        "SELECT id, tasks_path FROM projects WHERE id != ? ORDER BY id ASC",
    )
    .bind(id)
    .fetch_all(&mut *transaction)
    .await
    .map_err(|error| unexpected("reading project task locations", error))?
    .into_iter()
    .map(|(id, tasks_path)| {
        ProjectPrefix::try_new(id)
            .map(|id| (id, tasks_path))
            .map_err(|error| unexpected("converting project task location", error))
    })
    .collect::<Result<Vec<_>, _>>()?;
    task_location::reject_collision(&command.id, &candidate_path, existing, &command.home)
        .map_err(task_location_error)?;
    let update = sqlx::query!(
        r#"
        UPDATE projects
        SET paused_at = NULL
        WHERE id = ? AND paused_at IS NOT NULL
        "#,
        id,
    )
    .execute(&mut *transaction)
    .await
    .map_err(|error| unexpected("resuming project", error))?;
    let row = sqlx::query_as!(
        ProjectRow,
        r#"
        SELECT
            projects.id AS "id!",
            projects.title AS "title!",
            project_sources.kind AS "source_kind!",
            project_sources.value AS "source_value!",
            projects.tasks_kind AS "tasks_kind!",
            projects.tasks_path AS "tasks_path!",
            projects.created_at AS "created_at!",
            (projects.paused_at IS NOT NULL) AS "is_paused!: bool"
        FROM projects
        JOIN project_sources ON project_sources.id = projects.project_source_id
        WHERE projects.id = ?
        "#,
        id,
    )
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| unexpected("reading resumed project", error))?
    .ok_or(ResumeProjectError::ProjectNotFound {
        id: command.id.clone(),
    })?;
    let project = super::logic::project_from_row(row)
        .map_err(|error| unexpected_row("converting resumed project", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| unexpected("committing project resume", error))?;

    Ok(ProjectStateChange {
        project,
        changed: update.rows_affected() == 1,
    })
}

fn unexpected(
    context: &'static str,
    source: impl Error + Send + Sync + 'static,
) -> ResumeProjectError {
    ResumeProjectError::Unexpected {
        context,
        source: Box::new(source),
    }
}

fn unexpected_row(context: &'static str, source: ProjectRowError) -> ResumeProjectError {
    unexpected(context, source)
}

fn task_location_error(error: TaskLocationError) -> ResumeProjectError {
    match error {
        TaskLocationError::InvalidPath {
            project_id,
            path,
            source,
        } => ResumeProjectError::InvalidTaskPath {
            project_id,
            path,
            source,
        },
        TaskLocationError::Collision {
            first_id,
            second_id,
            path,
        } => ResumeProjectError::DuplicateRuntimeTaskLocation {
            first_id,
            second_id,
            path,
        },
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use pwf_models::project::ProjectPrefix;

    use super::*;
    use crate::ports::TestDatabase;

    #[tokio::test]
    async fn runtime_alias_of_other_paused_project_is_rejected() {
        let database = TestDatabase::new().await;
        let home = PathBuf::from("/home/tester");
        let resolved_path = home.join("tasks/shared");
        database
            .insert_project("PWF", "pwf", "/work/PWF", "~/tasks/shared", true)
            .await;
        database
            .insert_project(
                "ALT",
                "alt",
                "/work/ALT",
                &resolved_path.to_string_lossy(),
                true,
            )
            .await;

        let error = super::execute(
            ResumeProject {
                id: ProjectPrefix::try_new("PWF").unwrap(),
                home,
            },
            &database,
        )
        .await
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            format!(
                "managed projects ALT and PWF resolve to the same task location: {}",
                resolved_path.display()
            )
        );
        let paused_at: Option<String> =
            sqlx::query_scalar("SELECT paused_at FROM projects WHERE id = 'PWF'")
                .fetch_one(database.pool())
                .await
                .unwrap();
        assert!(paused_at.is_some());
    }
}
