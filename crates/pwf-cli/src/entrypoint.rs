use pwf_client::PwfClient;
use pwf_models::settings::{NoteStatusColors, ProjectStatusColors};

use crate::{command, console::Console, note, project, settings, task};

pub async fn run() {
    let parsed = command::parse_argv(std::env::args().skip(1).collect())
        .unwrap_or_else(|error| error.exit());
    match dispatch(parsed).await {
        Ok(output) => print_output(&output),
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

async fn dispatch(parsed: command::Cli) -> anyhow::Result<String> {
    if let command::RootCommand::Server(arguments) = parsed.command {
        return crate::server::run(arguments).await;
    }
    if matches!(
        &parsed.command,
        command::RootCommand::Project(arguments) if arguments.command.is_none()
    ) {
        return Ok(command::project_help());
    }

    let client = PwfClient::connect_local().await?;
    match parsed.command {
        command::RootCommand::Server(arguments) => crate::server::run(arguments).await,
        command::RootCommand::Project(arguments) => {
            let project_client = client.project();
            let colors = if matches!(&arguments.command, Some(project::Command::List(arguments)) if !arguments.json)
            {
                settings::load(&client.settings())
                    .await?
                    .project_status_colors()
            } else {
                ProjectStatusColors::default()
            };
            project::run(arguments, Console::from_terminal(), colors, &project_client).await
        }
        command::RootCommand::Task(command) => {
            let user_settings = settings::load(&client.settings()).await?;
            let task_client = client.task();
            task::run(
                &command,
                Console::from_terminal(),
                user_settings.task_status_colors(),
                &task_client,
                &client.project(),
            )
            .await
        }
        command::RootCommand::Note(arguments) => {
            let note_client = client.note();
            let colors = if matches!(&arguments.command, note::Command::List(_)) {
                settings::load(&client.settings())
                    .await?
                    .note_status_colors()
            } else {
                NoteStatusColors::default()
            };
            note::run(
                &arguments,
                Console::from_terminal(),
                colors,
                &note_client,
                &client.project(),
            )
            .await
        }
    }
}
