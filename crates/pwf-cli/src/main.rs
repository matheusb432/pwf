use pwf_application::ports::clock::Clock;
use pwf_cli::{command, note, project, task};
use pwf_infra::{clock::LocalClock, obsidian::ObsidianStore};
use pwf_models::project::HomeDirectory;

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

async fn run(parsed: command::Cli, clock: &impl Clock) -> anyhow::Result<String> {
    match parsed.command {
        command::RootCommand::Project(arguments) if arguments.command.is_none() => {
            Ok(command::project_help())
        }
        command::RootCommand::Project(arguments) => {
            let home = project_command_home(&arguments)?;
            let pool = open_database().await?;
            project::run(arguments, &pool, home).await
        }
        command::RootCommand::Task(command) => {
            let pool = open_database().await?;
            let home = managed_project_home()?;
            let store = ObsidianStore::new(home.clone());
            task::run(
                &command,
                pwf_cli::console::Console::from_terminal(),
                &store,
                &pool,
                &home,
                clock,
            )
            .await
        }
        command::RootCommand::Note(arguments) => {
            let pool = open_database().await?;
            let store = ObsidianStore::new(managed_project_home()?);
            note::run(&arguments, &store, &pool, clock).await
        }
    }
}

async fn open_database() -> anyhow::Result<sqlx::SqlitePool> {
    let path = pwf_infra::database::database_path()
        .map_err(|error| anyhow::anyhow!("resolving project database path failed: {error:#}"))?;
    let pool = pwf_infra::database::build_pool(&path)
        .await
        .map_err(|error| {
            anyhow::anyhow!(
                "opening project database {} failed: {error:#}",
                path.display()
            )
        })?;
    pwf_infra::database::check_database_ready(&pool)
        .await
        .map_err(|error| {
            anyhow::anyhow!(
                "project database {} failed its migration readiness check: {error:#}",
                path.display()
            )
        })?;
    Ok(pool)
}

fn project_command_home(arguments: &project::Arguments) -> anyhow::Result<Option<HomeDirectory>> {
    match arguments.command.as_ref() {
        Some(project::Command::Add(_)) => managed_project_home().map(Some),
        Some(project::Command::Rename(_) | project::Command::Resume(_)) => {
            managed_project_home().map(Some)
        }
        _ => Ok(None),
    }
}

fn managed_project_home() -> anyhow::Result<HomeDirectory> {
    directories::BaseDirs::new()
        .map(|directories| HomeDirectory::new(directories.home_dir().to_path_buf()))
        .ok_or_else(|| anyhow::anyhow!("resolving the home directory for managed projects failed"))
}
