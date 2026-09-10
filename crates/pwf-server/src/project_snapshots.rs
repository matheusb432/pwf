use std::time::Duration;

use pwf_application::project::{list_projects, refresh_project_snapshot};
use pwf_wire::project::ProjectStatusFilter;
use tokio::sync::watch;

use crate::AppState;

#[cfg(test)]
mod tests;

pub(crate) const REFRESH_INTERVAL: Duration = Duration::from_secs(60);

pub(crate) async fn run(state: AppState, interval: Duration, mut shutdown: watch::Receiver<bool>) {
    loop {
        if *shutdown.borrow() {
            return;
        }
        let projects = tokio::select! {
            biased;
            _ = shutdown.changed() => return,
            projects = list_projects::execute(ProjectStatusFilter::IncludingPaused, &state.pool) => projects,
        };
        let projects = match projects {
            Ok(projects) => projects,
            Err(error) => {
                tracing::warn!(%error, "listing projects for snapshot refresh failed");
                Vec::new()
            }
        };
        for project in projects
            .into_iter()
            .filter(|project| project.snapshot_enabled)
        {
            if *shutdown.borrow() || shutdown.has_changed().is_err() {
                return;
            }
            refresh_project(project, state.store.clone()).await;
        }
        tokio::select! {
            biased;
            _ = shutdown.changed() => return,
            () = tokio::time::sleep(interval) => {}
        }
    }
}

async fn refresh_project(
    project: pwf_models::project::Project,
    store: pwf_infra::obsidian::ObsidianStore,
) {
    let project_id = project.id.clone();
    let refreshed = tokio::task::spawn_blocking(move || {
        refresh_project_snapshot::execute(&project, &store, &store, &store)
    })
    .await;
    match refreshed {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            tracing::warn!(%project_id, %error, "project snapshot refresh failed");
        }
        Err(error) => {
            tracing::error!(%project_id, %error, "project snapshot worker failed");
        }
    }
}
