use pwf_client::{
    pb::{GetProjectRequest, ListProjectsRequest, ProjectStatusFilter},
    project::ProjectClient,
};
use pwf_models::project::ProjectId;

use crate::error::Error;

pub(crate) async fn resolve_project_id(
    id: &ProjectId,
    client: &ProjectClient,
) -> Result<ProjectId, Error> {
    let status = ProjectStatusFilter::ActiveOnly as i32;
    match client
        .get_project(GetProjectRequest {
            id: id.to_string(),
            status,
        })
        .await
    {
        Ok(response) => {
            return ProjectId::try_new(response.id).map_err(|error| {
                anyhow::anyhow!("pwf-server returned an invalid project ID: {error}").into()
            });
        }
        Err(error) if error.is_not_found() => {}
        Err(error) => return Err(error.into()),
    }
    let projects = client.list_projects(ListProjectsRequest { status }).await?;
    let ids = projects
        .projects
        .into_iter()
        .map(|project| ProjectId::try_new(project.id).map_err(anyhow::Error::from))
        .collect::<anyhow::Result<Vec<_>>>()?;
    Err(Error::ProjectNotFound {
        id: id.clone(),
        suggestion: suggest_project(id, &ids).cloned(),
    })
}

fn suggest_project<'a>(id: &ProjectId, candidates: &'a [ProjectId]) -> Option<&'a ProjectId> {
    let mut matches = candidates
        .iter()
        .filter(|candidate| strsim::damerau_levenshtein(id.as_ref(), candidate.as_ref()) == 1);
    let suggestion = matches.next()?;
    matches.next().is_none().then_some(suggestion)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggestions_cover_one_edit_and_suppress_ties() {
        let ids = ["ABC", "XYZ"].map(|id| id.parse().unwrap());
        for input in ["AC", "ABCD", "ABD", "BAC", "ACB"] {
            assert_eq!(
                suggest_project(&input.parse().unwrap(), &ids),
                Some(&ids[0]),
                "{input}"
            );
        }
        for input in ["ABC", "ZZ", "CAB"] {
            assert_eq!(
                suggest_project(&input.parse().unwrap(), &ids),
                None,
                "{input}"
            );
        }
        let ties = ["ABC", "ABD"].map(|id| id.parse().unwrap());
        assert_eq!(suggest_project(&"ABE".parse().unwrap(), &ties), None);
        assert_eq!(suggest_project(&"ABE".parse().unwrap(), &[]), None);
    }
}
