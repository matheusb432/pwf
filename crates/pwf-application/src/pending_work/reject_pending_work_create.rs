//! Rejects retired pending-work create forms after optional project resolution.

use super::{ProjectRegistry, ProjectResolutionError};

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
    ProjectResolution(#[from] ProjectResolutionError),
}

/// Resolves an optional project before returning the retired-create outcome.
///
/// # Errors
///
/// Returns [`RejectPendingWorkCreateError`] when the supplied project identifier does not resolve
/// uniquely.
#[cqrsy::query]
pub fn execute(
    query: &RejectPendingWorkCreate,
    projects: &ProjectRegistry,
) -> Result<RejectPendingWorkCreateOk, RejectPendingWorkCreateError> {
    if let Some(project_identifier) = query.project_identifier.as_deref() {
        projects.resolve(project_identifier)?;
    }

    Ok(RejectPendingWorkCreateOk)
}

#[cfg(test)]
mod tests {
    use pwf_models::pending_work::ProjectName;

    use super::*;

    fn projects() -> ProjectRegistry {
        ProjectRegistry::new([(
            ProjectName::try_new("pwf").unwrap(),
            Some("/repo/pwf".to_string()),
            Some("PWF".to_string()),
        )])
    }

    #[test]
    fn absent_or_resolved_project_reaches_rejection_outcome() {
        for project_identifier in [None, Some("PWF".to_string())] {
            let outcome =
                super::execute(&RejectPendingWorkCreate { project_identifier }, &projects())
                    .unwrap();

            assert_eq!(outcome, RejectPendingWorkCreateOk);
        }
    }

    #[test]
    fn unknown_project_fails_before_rejection_outcome() {
        let error = super::execute(
            &RejectPendingWorkCreate {
                project_identifier: Some("missing".to_string()),
            },
            &projects(),
        )
        .unwrap_err();

        assert!(matches!(
            error,
            RejectPendingWorkCreateError::ProjectResolution(
                ProjectResolutionError::Unknown { ref identifier, .. }
            ) if identifier == "missing"
        ));
    }
}
