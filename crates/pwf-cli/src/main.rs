use std::path::PathBuf;

use pwf_application::{
    pending_work::ProjectRegistry,
    ports::clock::Clock,
    project::load_active_projects::{self, ActiveProject, LoadActiveProjects},
};
use pwf_cli::{command, note, pending_work, project};
use pwf_infra::{
    clock::LocalClock,
    obsidian::{ObsidianProject, ObsidianStore},
};
use pwf_models::pending_work::ProjectIndexIdentity;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let clock = LocalClock;
    let parsed = command::parse_argv(std::env::args().skip(1).collect())
        .unwrap_or_else(|error| error.exit());
    match run(parsed, &clock).await {
        Ok(out) => {
            if !out.is_empty() {
                println!("{out}");
            }
        }
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    }
}

async fn run(parsed: command::Cli, clock: &impl Clock) -> Result<String, String> {
    match parsed.command {
        command::RootCommand::Project(arguments) if arguments.command.is_none() => {
            Ok(command::project_help())
        }
        command::RootCommand::Project(arguments) => {
            let home = project_command_home(&arguments)?;
            let pool = open_database().await?;
            project::run(arguments, &pool, home).await
        }
        command::RootCommand::PendingWork(command) => {
            let pool = open_database().await?;
            let projects = load_active_projects(&pool).await?;
            pending_work::run(
                &command,
                pwf_cli::console::Console::from_terminal(),
                &projects.store,
                &projects.registry,
                clock,
            )
        }
        command::RootCommand::Note(arguments) => {
            let pool = open_database().await?;
            let projects = load_active_projects(&pool).await?;
            note::run(&arguments, &projects.store, &projects.registry, clock)
        }
    }
}

struct ActiveProjects {
    registry: ProjectRegistry,
    store: ObsidianStore,
}

async fn open_database() -> Result<sqlx::SqlitePool, String> {
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
    Ok(pool)
}

async fn load_active_projects(pool: &sqlx::SqlitePool) -> Result<ActiveProjects, String> {
    let projects = load_active_projects::execute(
        LoadActiveProjects {
            home: managed_project_home()?,
        },
        pool,
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

fn project_command_home(arguments: &project::Arguments) -> Result<Option<PathBuf>, String> {
    match arguments.command.as_ref() {
        Some(project::Command::Add(_)) => managed_project_home().map(Some),
        Some(project::Command::Rename(_) | project::Command::Resume(_)) => {
            managed_project_home().map(Some)
        }
        _ => Ok(None),
    }
}

fn managed_project_home() -> Result<PathBuf, String> {
    directories::BaseDirs::new()
        .map(|directories| directories.home_dir().to_path_buf())
        .ok_or_else(|| "resolving the home directory for managed projects failed".to_string())
}
