//! Rejects retired pending-work create forms after optional project resolution.

use crate::project::{
    ProjectStatusFilter,
    resolve_project::{self, ResolveProject, ResolveProjectError},
};

/// Requests rejection of a retired create form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RejectPendingWorkCreate {
    /// Selects the project that must resolve before rejection, when present.
    pub project_identifier: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RejectPendingWorkCreateOk;

#[derive(Debug, thiserror::Error)]
pub enum RejectPendingWorkCreateError {
    #[error(transparent)]
    ProjectResolution(#[from] ResolveProjectError),
}

/// Resolves an optional project before returning the retired-create outcome.
///
/// # Errors
///
/// Returns [`RejectPendingWorkCreateError`] when the supplied project identifier does not resolve
/// uniquely.
#[cqrsy::query]
pub async fn execute(
    query: &RejectPendingWorkCreate,
    pool: &sqlx::SqlitePool,
) -> Result<RejectPendingWorkCreateOk, RejectPendingWorkCreateError> {
    if let Some(project_identifier) = query.project_identifier.as_deref() {
        resolve_project::execute(
            ResolveProject {
                identifier: project_identifier.to_string(),
                status: ProjectStatusFilter::ACTIVE,
            },
            pool,
        )
        .await?;
    }

    Ok(RejectPendingWorkCreateOk)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::insert_project;

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn absent_or_resolved_project_reaches_rejection_outcome(pool: sqlx::SqlitePool) {
        insert_project(&pool, "PWF", "pwf", "/repo/pwf", "/tasks/pwf", false).await;
        for project_identifier in [None, Some("PWF".to_string())] {
            let outcome = super::execute(&RejectPendingWorkCreate { project_identifier }, &pool)
                .await
                .unwrap();

            assert_eq!(outcome, RejectPendingWorkCreateOk);
        }
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn unknown_project_fails_before_rejection_outcome(pool: sqlx::SqlitePool) {
        let error = super::execute(
            &RejectPendingWorkCreate {
                project_identifier: Some("missing".to_string()),
            },
            &pool,
        )
        .await
        .unwrap_err();

        assert!(matches!(
            error,
            RejectPendingWorkCreateError::ProjectResolution(
                ResolveProjectError::Unknown { ref identifier, .. }
            ) if identifier == "missing"
        ));
    }
}
