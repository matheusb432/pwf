use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
};

use pwf::{command, engines};
use pwf_application::{pending_work::ProjectRegistry, project::Project};
use pwf_domain::{pending_work::ProjectIndexIdentity, project::ProjectPrefix};
use pwf_infra::{
    SqliteStore,
    obsidian::{ObsidianProject, ObsidianStore},
};

mod runtime_path;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let argv = normalize_rich_help_aliases(argv);

    match command::parse_argv(argv) {
        Ok(parsed) => match run(parsed).await {
            Ok(out) => {
                if !out.is_empty() {
                    println!("{out}");
                }
            }
            Err(e) => {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        },
        Err(e) => exit_with_clap_error(&e),
    }
}

fn exit_with_clap_error(e: &clap::Error) -> ! {
    let rendered = e.render().ansi().to_string();
    if e.use_stderr() {
        let _ = anstream::stderr().write_all(rendered.as_bytes());
    } else {
        let _ = anstream::stdout().write_all(rendered.as_bytes());
    }
    std::process::exit(e.exit_code());
}

async fn run(parsed: command::Cli) -> Result<String, String> {
    match parsed.engine {
        command::Engine::Project(arguments) if arguments.command.is_none() => {
            Ok(command::project_help())
        }
        command::Engine::Project(arguments) => {
            let home = project_command_home(&arguments)?;
            let database = open_database().await?;
            if let Some(home) = home {
                validate_project_command(&arguments, &database, &home).await?;
            }
            engines::project::run(arguments, &database).await
        }
        command::Engine::PendingWork(command) => {
            let database = open_database().await?;
            let projects = load_active_projects(&database).await?;
            engines::pending_work::run(
                &command,
                pwf::console::Console::from_terminal(),
                &projects.store,
                &projects.registry,
            )
        }
        command::Engine::Handoff { command } => match command {
            engines::handoff::Command::Add(_) => {
                let database = open_database().await?;
                let projects = load_active_projects(&database).await?;
                engines::handoff::run(&command, &projects.store, &projects.registry)
            }
            engines::handoff::Command::List(_) => engines::handoff::run(
                &command,
                &ObsidianStore::new([]),
                &ProjectRegistry::default(),
            ),
        },
        command::Engine::Note(arguments) => {
            let database = open_database().await?;
            let projects = load_active_projects(&database).await?;
            engines::note::run(&arguments, &projects.store, &projects.registry)
        }
    }
}

struct ActiveProjects {
    registry: ProjectRegistry,
    store: ObsidianStore,
}

async fn open_database() -> Result<SqliteStore, String> {
    let path = pwf_infra::database::database_path()
        .map_err(|error| format!("resolving project database path failed: {error}"))?;
    let pool = pwf_infra::database::build_pool(&path)
        .await
        .map_err(|error| {
            format!(
                "opening project database {} failed: {error}",
                path.display()
            )
        })?;
    pwf_infra::database::migrate_database(&pool)
        .await
        .map_err(|error| {
            format!(
                "migrating project database {} failed: {error}",
                path.display()
            )
        })?;
    Ok(SqliteStore::new(pool))
}

async fn load_active_projects(database: &SqliteStore) -> Result<ActiveProjects, String> {
    let projects = pwf_application::project::list::execute(
        pwf_application::project::list::ListProjects {
            include_paused: false,
        },
        database,
    )
    .await
    .map_err(|error| format!("listing active projects failed: {error}"))?;
    let home = managed_project_home()?;
    compose_active_projects(&projects, &home)
}

fn compose_active_projects(projects: &[Project], home: &Path) -> Result<ActiveProjects, String> {
    let mut task_path_owners = BTreeMap::new();
    let mut resolved_projects = Vec::with_capacity(projects.len());

    for project in projects {
        let source =
            resolve_project_path(&project.id, "source", project.source.value().as_ref(), home)?;
        let tasks = resolve_project_path(&project.id, "task", project.tasks.path().as_ref(), home)?;
        if let Some(existing_id) =
            task_path_owners.insert(tasks.identity().clone(), project.id.clone())
        {
            return Err(task_path_conflict(&existing_id, &project.id, tasks.path()));
        }
        resolved_projects.push((project, source, tasks));
    }

    let registry = ProjectRegistry::new(resolved_projects.iter().map(|(project, source, _)| {
        (
            project.title.clone(),
            Some(source.path().to_string_lossy().into_owned()),
            Some(project.id.to_string()),
        )
    }));
    let store = ObsidianStore::new(resolved_projects.iter().map(|(project, _, tasks)| {
        ObsidianProject::new(
            ProjectIndexIdentity::new(project.id.clone(), project.title.clone()),
            tasks.path().to_path_buf(),
        )
    }));

    Ok(ActiveProjects { registry, store })
}

fn project_command_home(
    arguments: &engines::project::Arguments,
) -> Result<Option<PathBuf>, String> {
    match arguments.command.as_ref() {
        Some(engines::project::Command::Add(arguments)) => {
            let home = managed_project_home()?;
            let project = &arguments.payload.0;
            resolve_project_path(&project.id, "task", project.tasks.path().as_ref(), &home)?;
            Ok(Some(home))
        }
        Some(engines::project::Command::Resume(_)) => managed_project_home().map(Some),
        _ => Ok(None),
    }
}

async fn validate_project_command(
    arguments: &engines::project::Arguments,
    database: &SqliteStore,
    home: &Path,
) -> Result<(), String> {
    let projects = pwf_application::project::list::execute(
        pwf_application::project::list::ListProjects {
            include_paused: true,
        },
        database,
    )
    .await
    .map_err(|error| format!("listing managed projects for task validation failed: {error}"))?;

    let candidate = match arguments.command.as_ref() {
        Some(engines::project::Command::Add(arguments)) => Some((
            &arguments.payload.0.id,
            arguments.payload.0.tasks.path().as_ref(),
        )),
        Some(engines::project::Command::Resume(arguments)) => projects
            .iter()
            .find(|project| project.id == arguments.id)
            .map(|project| (&project.id, project.tasks.path().as_ref())),
        _ => None,
    };
    let Some((candidate_id, candidate_path)) = candidate else {
        return Ok(());
    };
    let candidate = resolve_project_path(candidate_id, "task", candidate_path, home)?;

    for project in projects
        .iter()
        .filter(|project| project.id != *candidate_id)
    {
        let existing =
            resolve_project_path(&project.id, "task", project.tasks.path().as_ref(), home)?;
        if candidate.identity() == existing.identity() {
            return Err(task_path_conflict(
                candidate_id,
                &project.id,
                candidate.path(),
            ));
        }
    }

    Ok(())
}

fn managed_project_home() -> Result<PathBuf, String> {
    directories::BaseDirs::new()
        .map(|directories| directories.home_dir().to_path_buf())
        .ok_or_else(|| "resolving the home directory for managed projects failed".to_string())
}

fn resolve_project_path(
    project_id: &ProjectPrefix,
    field: &'static str,
    path: &str,
    home: &Path,
) -> Result<runtime_path::ResolvedPath, String> {
    runtime_path::resolve(path, home).map_err(|error| {
        format!("managed project {project_id} {field} path '{path}' is invalid: {error}")
    })
}

fn task_path_conflict(
    first_id: &ProjectPrefix,
    second_id: &ProjectPrefix,
    path: &std::path::Path,
) -> String {
    let mut project_ids = [first_id.to_string(), second_id.to_string()];
    project_ids.sort();
    format!(
        "managed projects {} and {} resolve to the same task location: {}",
        project_ids[0],
        project_ids[1],
        path.display()
    )
}

fn normalize_rich_help_aliases(argv: Vec<String>) -> Vec<String> {
    if argv.is_empty() || argv.first().is_some_and(|arg| arg == "--list") {
        return vec!["--help".to_string()];
    }
    argv
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use pwf_application::project::{add::AddProject, pause::PauseProject};
    use pwf_domain::project::{
        ProjectName, ProjectPrefix, ProjectSource, ProjectSourceKind, ProjectSourceValue,
        ProjectTasks, ProjectTasksKind, ProjectTasksPath,
    };

    use super::{compose_active_projects, load_active_projects};

    fn project(id: &str, title: &str, source: &str, tasks_path: &str) -> AddProject {
        AddProject {
            id: ProjectPrefix::try_new(id).unwrap(),
            title: ProjectName::try_new(title).unwrap(),
            source: ProjectSource::new(
                ProjectSourceKind::Directory,
                ProjectSourceValue::try_new(source).unwrap(),
            ),
            tasks: ProjectTasks::new(
                ProjectTasksKind::Directory,
                ProjectTasksPath::try_new(tasks_path).unwrap(),
            ),
        }
    }

    #[tokio::test]
    async fn active_project_composition_expands_runtime_paths_without_changing_stored_values() {
        let directory = tempfile::tempdir().unwrap();
        let pool = pwf_infra::database::build_pool(&directory.path().join("projects.sqlite3"))
            .await
            .unwrap();
        pwf_infra::database::migrate_database(&pool).await.unwrap();
        let database = pwf_infra::SqliteStore::new(pool);
        pwf_application::project::add::execute(
            project("pwf", "pwf", "~/tools/pwf", "~/tasks/pwf"),
            &database,
        )
        .await
        .unwrap();
        pwf_application::project::add::execute(
            project("arc", "repository", "/work/repository", "/tasks/repository"),
            &database,
        )
        .await
        .unwrap();
        pwf_application::project::pause::execute(
            PauseProject {
                id: ProjectPrefix::try_new("arc").unwrap(),
            },
            &database,
        )
        .await
        .unwrap();

        let runtime = load_active_projects(&database).await.unwrap();
        let home = directories::BaseDirs::new()
            .unwrap()
            .home_dir()
            .to_path_buf();
        let pwf = runtime.registry.resolve("PWF").unwrap();
        assert_eq!(
            runtime.registry.repo_for(pwf),
            Some(home.join("tools/pwf").to_string_lossy().as_ref())
        );
        assert_eq!(
            runtime.store.tasks_path(pwf).unwrap(),
            home.join("tasks/pwf")
        );
        assert!(runtime.registry.resolve("ARC").is_err());

        let project = pwf_application::project::get::execute(
            pwf_application::project::get::GetProject {
                id: ProjectPrefix::try_new("pwf").unwrap(),
            },
            &database,
        )
        .await
        .unwrap();
        assert_eq!(project.source.value().as_ref(), "~/tools/pwf");
        assert_eq!(project.tasks.path().as_ref(), "~/tasks/pwf");
    }

    #[test]
    fn active_project_composition_rejects_runtime_task_path_aliases() {
        let projects = [
            persisted_project("pwf", "pwf", "/work/pwf", "~/tasks/shared"),
            persisted_project("alt", "other", "/work/other", "/home/developer/tasks/shared"),
        ];

        let Err(error) = compose_active_projects(&projects, Path::new("/home/developer")) else {
            panic!("runtime task path aliases were accepted");
        };

        assert_eq!(
            error,
            "managed projects ALT and PWF resolve to the same task location: \
             /home/developer/tasks/shared"
        );
    }

    fn persisted_project(
        id: &str,
        title: &str,
        source: &str,
        tasks_path: &str,
    ) -> pwf_application::project::Project {
        pwf_application::project::Project {
            id: ProjectPrefix::try_new(id).unwrap(),
            title: ProjectName::try_new(title).unwrap(),
            source: ProjectSource::new(
                ProjectSourceKind::Directory,
                ProjectSourceValue::try_new(source).unwrap(),
            ),
            tasks: ProjectTasks::new(
                ProjectTasksKind::Directory,
                ProjectTasksPath::try_new(tasks_path).unwrap(),
            ),
            created_at: "2026-07-25T00:00:00.000Z".to_string(),
            is_paused: false,
        }
    }
}
