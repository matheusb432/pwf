//! Infers the managed project that owns an editor context path.

use std::path::{Path, PathBuf};

use pwf_client::{ClientError, pb, project::ProjectClient};

/// A registered directory that claims every path below it for one project.
#[derive(Debug)]
struct ProjectRoot {
    project_id: String,
    path: PathBuf,
}

/// Returns the active project owning the first context path that a project directory contains.
pub(crate) async fn infer_project(
    projects: &ProjectClient,
    context_paths: &[PathBuf],
) -> Result<Option<String>, ClientError> {
    let response = projects
        .list_projects(pb::ListProjectsRequest {
            status: pb::ProjectStatusFilter::ActiveOnly as i32,
        })
        .await?;
    let home = std::env::home_dir();
    let roots = project_roots(response.projects, home.as_deref());
    let context_paths = context_paths
        .iter()
        .map(|path| canonical_path(path))
        .collect::<Vec<_>>();
    Ok(select_project(&roots, &context_paths).map(str::to_owned))
}

fn project_roots(projects: Vec<pb::Project>, home: Option<&Path>) -> Vec<ProjectRoot> {
    projects
        .into_iter()
        .flat_map(|project| {
            let source = project
                .source_value
                .filter(|_| project.source_kind.as_deref() == Some("directory"));
            [Some(project.tasks_path), source]
                .into_iter()
                .flatten()
                .filter_map(|raw| expand_home(&raw, home))
                .map(|path| ProjectRoot {
                    project_id: project.id.clone(),
                    path: canonical_path(&path),
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Checks context paths in priority order; the deepest containing root wins for each path.
fn select_project<'roots>(
    roots: &'roots [ProjectRoot],
    context_paths: &[PathBuf],
) -> Option<&'roots str> {
    context_paths.iter().find_map(|context| {
        roots
            .iter()
            .filter(|root| context.starts_with(&root.path))
            .max_by_key(|root| root.path.components().count())
            .map(|root| root.project_id.as_str())
    })
}

/// Resolves the registry's `~` shorthand the same way the server resolves project paths.
fn expand_home(raw: &str, home: Option<&Path>) -> Option<PathBuf> {
    if raw == "~" {
        return home.map(Path::to_path_buf);
    }
    match raw.strip_prefix("~/").or_else(|| raw.strip_prefix("~\\")) {
        Some(remainder) => home.map(|home| home.join(remainder)),
        None => Some(PathBuf::from(raw)),
    }
}

/// Resolves symlinks so editor paths and registry paths compare equal; missing paths stay lexical.
fn canonical_path(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(id: &str, tasks_path: &str, source_path: Option<&str>) -> pb::Project {
        pb::Project {
            id: id.to_string(),
            tasks_path: tasks_path.to_string(),
            source_kind: source_path.map(|_| "directory".to_string()),
            source_value: source_path.map(str::to_string),
            ..Default::default()
        }
    }

    #[test]
    fn earlier_context_path_wins_over_the_working_directory() {
        let roots = project_roots(
            vec![
                project("NOTE", "/vault/notes", None),
                project("CODE", "/vault/code-tasks", Some("/src/code")),
            ],
            None,
        );

        let project = select_project(
            &roots,
            &[
                PathBuf::from("/vault/notes/NOTE-0001.md"),
                PathBuf::from("/src/code/crates"),
            ],
        );

        assert_eq!(project, Some("NOTE"));
    }

    #[test]
    fn deepest_project_directory_owns_nested_paths() {
        let roots = project_roots(
            vec![
                project("MONO", "/tasks/mono", Some("/src")),
                project("PART", "/tasks/part", Some("/src/part")),
            ],
            None,
        );

        assert_eq!(
            select_project(&roots, &[PathBuf::from("/src/part/lib.rs")]),
            Some("PART")
        );
        assert_eq!(
            select_project(&roots, &[PathBuf::from("/src/other/lib.rs")]),
            Some("MONO")
        );
        assert_eq!(select_project(&roots, &[PathBuf::from("/elsewhere")]), None);
    }

    #[test]
    fn home_shorthand_resolves_against_the_home_directory() {
        let roots = project_roots(
            vec![project("HOME", "~/tasks/home", None)],
            Some(Path::new("/users/me")),
        );

        assert_eq!(
            select_project(
                &roots,
                &[PathBuf::from("/users/me/tasks/home/HOME-0001.md")]
            ),
            Some("HOME")
        );
    }
}
