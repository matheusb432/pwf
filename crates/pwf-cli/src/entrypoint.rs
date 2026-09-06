use pwf_client::PwfClient;

use crate::{command, note, project, settings, task};

pub async fn run() {
    let parsed = command::parse_argv(std::env::args().skip(1).collect())
        .unwrap_or_else(|error| error.exit());
    match dispatch(parsed).await {
        Ok(output) => print_output(&output),
        Err(error) => {
            eprintln!("Error: {error}");
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
    if matches!(
        &parsed.command,
        command::RootCommand::Project(arguments) if arguments.command.is_none()
    ) {
        return Ok(command::project_help());
    }

    let client = PwfClient::connect_local().await?;
    match parsed.command {
        command::RootCommand::Project(arguments) => {
            let project_client = client.project();
            project::run(arguments, &project_client).await
        }
        command::RootCommand::Task(command) => {
            let user_settings = settings::load(&client.settings()).await?;
            let task_client = client.task();
            task::run(
                &command,
                crate::console::Console::from_terminal(),
                user_settings.task_status_colors(),
                &task_client,
            )
            .await
        }
        command::RootCommand::Note(arguments) => {
            let note_client = client.note();
            note::run(
                &arguments,
                crate::console::Console::from_terminal(),
                &note_client,
            )
            .await
        }
    }
}
