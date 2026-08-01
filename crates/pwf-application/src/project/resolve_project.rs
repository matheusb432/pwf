use std::error::Error;

use pwf_models::project::ProjectId;

use super::{
    Project, ProjectStatusFilter,
    dto::{ProjectRow, ProjectRowError},
    get_project::{self, GetProject, GetProjectError},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveProject {
    pub identifier: String,
    pub status: ProjectStatusFilter,
}

#[derive(Debug, thiserror::Error)]
pub enum ResolveProjectError {
    #[error(
        "Unknown managed project identifier: {identifier}\nManaged project identifiers: {}",
        known.join(", ")
    )]
    Unknown {
        identifier: String,
        known: Vec<String>,
    },
    #[error("{context}: {source}")]
    Unexpected {
        context: &'static str,
        #[source]
        source: Box<dyn Error + Send + Sync>,
    },
}

#[cqrsy::query]
pub async fn execute(
    query: ResolveProject,
    pool: &sqlx::SqlitePool,
) -> Result<Project, ResolveProjectError> {
    if let Some(project) = find_by_title(&query, pool).await? {
        return Ok(project);
    }

    if let Ok(id) = ProjectId::try_new(&query.identifier) {
        match get_project::execute(
            GetProject {
                id,
                status: query.status,
            },
            pool,
        )
        .await
        {
            Ok(project) => return Ok(project),
            Err(GetProjectError::ProjectNotFound { .. }) => {}
            Err(GetProjectError::Unexpected { context, source }) => {
                return Err(ResolveProjectError::Unexpected { context, source });
            }
        }
    }

    Err(ResolveProjectError::Unknown {
        known: known_project_names(query.status, pool).await?,
        identifier: query.identifier,
    })
}

async fn find_by_title(
    query: &ResolveProject,
    pool: &sqlx::SqlitePool,
) -> Result<Option<Project>, ResolveProjectError> {
    let identifier = &query.identifier;
    let includes_paused = query.status.includes_paused();
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
        WHERE projects.title = ?
          AND (? OR projects.paused_at IS NULL)
        "#,
        identifier,
        includes_paused,
    )
    .fetch_optional(pool)
    .await
    .map_err(|error| unexpected("resolving project by title", error))?;

    row.map(super::logic::project_from_row)
        .transpose()
        .map_err(|error| unexpected_row("converting resolved project", error))
}

async fn known_project_names(
    status: ProjectStatusFilter,
    pool: &sqlx::SqlitePool,
) -> Result<Vec<String>, ResolveProjectError> {
    let includes_paused = status.includes_paused();
    sqlx::query_scalar!(
        r#"
        SELECT title AS "title!"
        FROM projects
        WHERE ? OR paused_at IS NULL
        ORDER BY title ASC
        "#,
        includes_paused,
    )
    .fetch_all(pool)
    .await
    .map_err(|error| unexpected("listing known project identifiers", error))
}

fn unexpected(
    context: &'static str,
    source: impl Error + Send + Sync + 'static,
) -> ResolveProjectError {
    ResolveProjectError::Unexpected {
        context,
        source: Box::new(source),
    }
}

fn unexpected_row(context: &'static str, source: ProjectRowError) -> ResolveProjectError {
    unexpected(context, source)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::insert_project;

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn title_match_precedes_project_id_match(pool: sqlx::SqlitePool) {
        insert_project(&pool, "PWF", "alt", "/work/pwf", "/tasks/pwf", false).await;
        insert_project(&pool, "ALT", "other", "/work/alt", "/tasks/alt", false).await;

        let project = super::execute(
            ResolveProject {
                identifier: "ALT".to_string(),
                status: ProjectStatusFilter::ACTIVE,
            },
            &pool,
        )
        .await
        .unwrap();

        assert_eq!(project.id, ProjectId::try_new("PWF").unwrap());
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn active_resolution_excludes_paused_projects_from_matches_and_known_names(
        pool: sqlx::SqlitePool,
    ) {
        insert_project(&pool, "PWF", "pwf", "/work/pwf", "/tasks/pwf", true).await;
        insert_project(&pool, "ALT", "other", "/work/alt", "/tasks/alt", false).await;

        let error = super::execute(
            ResolveProject {
                identifier: "pwf".to_string(),
                status: ProjectStatusFilter::ACTIVE,
            },
            &pool,
        )
        .await
        .unwrap_err();

        assert!(matches!(
            error,
            ResolveProjectError::Unknown { identifier, known }
                if identifier == "pwf" && known == ["other"]
        ));
    }
}
