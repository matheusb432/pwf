use pwf_client::PwfClient;
use pwf_models::settings::{NoteStatusColors, ProjectStatusColors};

use crate::{command, console::Console, note, project, settings, task};

pub async fn run() {
    let parsed = command::parse_argv(std::env::args().skip(1).collect())
        .unwrap_or_else(|error| error.exit());
    let console = Console::from_terminal();
    match dispatch(parsed, console).await {
        Ok((output, success)) => {
            print_output(&output);
            if !success {
                std::process::exit(1);
            }
        }
        Err(error) => {
            eprintln!("Error: {error:#}");
            std::process::exit(1);
        }
    }
}

fn print_output(output: &str) {
    if !output.is_empty() {
        println!("{output}");
    }
}

async fn dispatch(parsed: command::Cli, console: Console) -> anyhow::Result<(String, bool)> {
    let output = match parsed.command {
        command::RootCommand::Doctor(arguments) => {
            let report = crate::doctor::inspect().await;
            return Ok((arguments.render(&report, console)?, !report.failed()));
        }
        command::RootCommand::Server(arguments) => crate::server::run(arguments, console).await,
        command::RootCommand::Project(arguments) if arguments.command.is_none() => {
            Ok(command::project_help())
        }
        command::RootCommand::Project(arguments) => {
            let client = connect().await?;
            let project_client = client.project();
            let colors = if matches!(&arguments.command, Some(project::Command::List(arguments)) if !arguments.json)
            {
                settings::load(&client.settings())
                    .await?
                    .project_status_colors()
            } else {
                ProjectStatusColors::default()
            };
            project::run(arguments, console, colors, &project_client).await
        }
        command::RootCommand::Task(command) => {
            let client = connect().await?;
            let user_settings = settings::load(&client.settings()).await?;
            let task_client = client.task();
            task::run(
                &command,
                console,
                user_settings.task_status_colors(),
                &task_client,
                &client.project(),
            )
            .await
        }
        command::RootCommand::Note(arguments) => {
            let client = connect().await?;
            let note_client = client.note();
            let colors = if matches!(&arguments.command, note::Command::List(_)) {
                settings::load(&client.settings())
                    .await?
                    .note_status_colors()
            } else {
                NoteStatusColors::default()
            };
            note::run(&arguments, console, colors, &note_client, &client.project()).await
        }
    }?;
    Ok((output, true))
}

async fn connect() -> anyhow::Result<PwfClient> {
    PwfClient::connect_local().await.map_err(|_| {
        anyhow::anyhow!(
            "Cannot connect to pwf-server.\nRun `{}` to identify the cause and recovery action.",
            command::DOCTOR_COMMAND
        )
    })
}
