use pwf_cli::{command, note, project, task};
use pwf_client::PwfClient;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let parsed = command::parse_argv(std::env::args().skip(1).collect())
        .unwrap_or_else(|error| error.exit());
    match run(parsed).await {
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

async fn run(parsed: command::Cli) -> anyhow::Result<String> {
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
            let task_client = client.task();
            task::run(
                &command,
                pwf_cli::console::Console::from_terminal(),
                &task_client,
            )
            .await
        }
        command::RootCommand::Note(arguments) => {
            let note_client = client.note();
            note::run(&arguments, &note_client).await
        }
    }
}
