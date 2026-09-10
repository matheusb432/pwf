use pwf_models::project::{ProjectId, ProjectSelector};
use pwf_wire::project::ResolveProject;

#[derive(Debug, thiserror::Error)]
pub enum ResolveProjectError {
    #[error("Unknown managed project identifier: {selector}\nManaged project identifiers: {known}")]
    ProjectNotFound {
        selector: ProjectSelector,
        known: String,
    },
    #[error("resolving managed project: {0}")]
    Database(#[from] sqlx::Error),
    #[error("persisted project ID is invalid: {0}")]
    InvalidProjectId(#[from] pwf_models::project::ProjectIdError),
}

#[cqrsy::query]
pub async fn execute(
    query: ResolveProject,
    pool: &sqlx::SqlitePool,
) -> Result<ProjectId, ResolveProjectError> {
    let selector = query.selector.as_ref();
    let id = query.selector.project_id().map(AsRef::as_ref);
    let includes_paused = query.status.includes_paused();
    let selected = sqlx::query_scalar!(
        r#"
        SELECT id AS "id!" FROM (
            SELECT id, 0 AS precedence FROM projects
            WHERE title = ? AND (? OR paused_at IS NULL)
            UNION ALL
            SELECT id, 1 AS precedence FROM projects
            WHERE id = ? AND (? OR paused_at IS NULL)
        )
        ORDER BY precedence
        LIMIT 1
        "#,
        selector,
        includes_paused,
        id,
        includes_paused,
    )
    .fetch_optional(pool)
    .await?;
    if let Some(id) = selected {
        return ProjectId::try_new(id).map_err(Into::into);
    }
    let titles = sqlx::query_scalar!(
        "SELECT title FROM projects WHERE (? OR paused_at IS NULL) ORDER BY title ASC",
        includes_paused,
    )
    .fetch_all(pool)
    .await?;
    Err(ResolveProjectError::ProjectNotFound {
        selector: query.selector,
        known: titles.join(", "),
    })
}
