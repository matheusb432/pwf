use pwf_client::{
    pb::{ListProjectsRequest, Project, ProjectStatusFilter},
    project::ProjectClient,
};
use pwf_models::project::{ProjectId, ProjectSelector};

// TODO: refactor naive logic that fetches all projects to resolve a single one, create rpc for
// resolving it. (maybe cache projects in memory, too?)
pub(crate) async fn resolve_project_id(
    selector: &ProjectSelector,
    client: &ProjectClient,
) -> anyhow::Result<ProjectId> {
    let projects = client
        .list_projects(ListProjectsRequest {
            status: ProjectStatusFilter::ActiveOnly as i32,
        })
        .await
        .map_err(crate::rpc_error)?
        .projects;
    select_project_id(selector, &projects)
}

fn select_project_id(
    selector: &ProjectSelector,
    projects: &[Project],
) -> anyhow::Result<ProjectId> {
    let selected = projects
        .iter()
        .find(|project| project.title.eq_ignore_ascii_case(selector.as_ref()))
        .or_else(|| {
            let id = selector.project_id()?;
            projects.iter().find(|project| project.id == id.as_ref())
        });
    if let Some(project) = selected {
        return ProjectId::try_new(&project.id).map_err(|error| {
            anyhow::anyhow!("pwf-server returned an invalid project ID: {error}")
        });
    }
    let known = projects
        .iter()
        .map(|project| project.title.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    Err(anyhow::anyhow!(
        "Unknown managed project identifier: {selector}\nManaged project identifiers: {known}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(id: &str, title: &str) -> Project {
        Project {
            id: id.to_string(),
            title: title.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn title_match_precedes_project_id_match() {
        let projects = [project("ALT", "other"), project("FOO", "alt")];
        for selector in ["alt", "ALT", " Alt "] {
            assert_eq!(
                select_project_id(&selector.parse().unwrap(), &projects)
                    .unwrap()
                    .as_ref(),
                "FOO"
            );
        }
    }

    #[test]
    fn ids_and_project_names_resolve_to_the_same_identity() {
        let projects = [project("FOO", "foo-bar")];
        for selector in ["foo", "FOO", "foo-bar", "FOO-BAR"] {
            assert_eq!(
                select_project_id(&selector.parse().unwrap(), &projects)
                    .unwrap()
                    .as_ref(),
                "FOO"
            );
        }
    }

    #[test]
    fn unknown_selector_reports_the_available_project_names() {
        let projects = [project("FOO", "foo-bar")];
        let error = select_project_id(&"missing".parse().unwrap(), &projects).unwrap_err();
        assert!(error.to_string().contains("missing"));
        assert!(error.to_string().contains("foo-bar"));
    }

    #[test]
    fn invalid_response_identity_is_a_protocol_error() {
        let error = select_project_id(
            &"foo-bar".parse().unwrap(),
            &[project("invalid", "foo-bar")],
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("pwf-server returned an invalid project ID")
        );
    }
}
