use std::error::Error;

use pwf_domain::project::ProjectPrefix;

use super::{
    Project, ProjectStateChange,
    dto::{ProjectRow, ProjectRowError},
};
use crate::AppDbStore;

/// Requests resuming one managed project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumeProject {
    /// Project prefix.
    pub id: ProjectPrefix,
}

/// Reports a missing project or unexpected resume failure.
#[derive(Debug, thiserror::Error)]
pub enum ResumeProjectError {
    /// The requested project does not exist.
    #[error("project not found: {id}")]
    ProjectNotFound {
        /// Missing project prefix.
        id: ProjectPrefix,
    },
    /// Project resuming failed outside expected absence.
    #[error("{context}: {source}")]
    Unexpected {
        /// Failed operation boundary.
        context: &'static str,
        /// Concrete database or persisted-data failure.
        #[source]
        source: Box<dyn Error + Send + Sync>,
    },
}

/// Resumes one project and reports whether persisted state changed.
///
/// # Errors
///
/// Returns [`ResumeProjectError::ProjectNotFound`] when no project has the requested ID. Returns
/// [`ResumeProjectError::Unexpected`] for database and persisted-data failures.
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
    .ok_or(ResumeProjectError::ProjectNotFound { id: command.id })?;
    let project = Project::try_from(row)
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
