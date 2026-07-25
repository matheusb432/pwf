use std::{
    io::Write,
    path::{Path, PathBuf},
};

use pwf::{command, engines};
use pwf_application::{
    Clock,
    pending_work::ProjectRegistry,
    project::{
        load_active::{self, ActiveProject, LoadActiveProjects},
        resolve_runtime_path::{self, ResolveRuntimePath, ResolvedPath},
    },
};
use pwf_domain::{pending_work::ProjectIndexIdentity, project::ProjectPrefix};
use pwf_infra::{
    SqliteStore,
    clock::LocalClock,
    obsidian::{ObsidianProject, ObsidianStore},
};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let argv = normalize_rich_help_aliases(argv);

    let clock = LocalClock;
    match command::parse_argv(argv) {
        Ok(parsed) => match run(parsed, &clock).await {
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

async fn run<C>(parsed: command::Cli, clock: &C) -> Result<String, String>
where
    C: Clock,
{
    match parsed.engine {
        command::Engine::Project(arguments) if arguments.command.is_none() => {
            Ok(command::project_help())
        }
        command::Engine::Project(arguments) => {
            let home = project_command_home(&arguments)?;
            let database = open_database().await?;
            engines::project::run(arguments, &database, home).await
        }
        command::Engine::PendingWork(command) => {
            let database = open_database().await?;
            let projects = load_active_projects(&database).await?;
            engines::pending_work::run(
                &command,
                pwf::console::Console::from_terminal(),
                &projects.store,
                &projects.registry,
                clock,
            )
        }
        command::Engine::Handoff { command } => match command {
            engines::handoff::Command::Add(_) => {
                let database = open_database().await?;
                let projects = load_active_projects(&database).await?;
                engines::handoff::run(&command, &projects.store, &projects.registry, clock)
            }
            engines::handoff::Command::List(_) => engines::handoff::run(
                &command,
                &ObsidianStore::new([]),
                &ProjectRegistry::default(),
                clock,
            ),
        },
        command::Engine::Note(arguments) => {
            let database = open_database().await?;
            let projects = load_active_projects(&database).await?;
            engines::note::run(&arguments, &projects.store, &projects.registry, clock)
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
    let projects = load_active::execute(
        LoadActiveProjects {
            home: managed_project_home()?,
        },
        database,
    )
    .await
    .map_err(|error| error.to_string())?;
    Ok(compose_active_projects(&projects))
}

fn compose_active_projects(projects: &[ActiveProject]) -> ActiveProjects {
    let registry = ProjectRegistry::new(projects.iter().map(|runtime| {
        (
            runtime.project.title.clone(),
            Some(runtime.source_path.to_string_lossy().into_owned()),
            Some(runtime.project.id.to_string()),
        )
    }));
    let store = ObsidianStore::new(projects.iter().map(|runtime| {
        ObsidianProject::new(
            ProjectIndexIdentity::new(runtime.project.id.clone(), runtime.project.title.clone()),
            runtime.tasks_path.clone(),
        )
    }));

    ActiveProjects { registry, store }
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
) -> Result<ResolvedPath, String> {
    resolve_runtime_path::execute(&ResolveRuntimePath {
        path: path.to_string(),
        home: home.to_path_buf(),
    })
    .map_err(|error| {
        format!("managed project {project_id} {field} path '{path}' is invalid: {error}")
    })
}

fn normalize_rich_help_aliases(argv: Vec<String>) -> Vec<String> {
    if argv.is_empty() || argv.first().is_some_and(|arg| arg == "--list") {
        return vec!["--help".to_string()];
    }
    argv
}
