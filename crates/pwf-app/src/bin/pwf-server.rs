use clap::{Parser, Subcommand};
use pwf_cli::{
    command::{DOCTOR_COMMAND, SERVER_BINARY_COMMAND, SERVER_DOCTOR_COMMAND},
    console::Console,
    doctor,
};

/// Run the local pwf background server in the foreground.
#[derive(Parser)]
#[command(name = SERVER_BINARY_COMMAND.name(), version)]
struct Arguments {
    #[arg(long, hide = true)]
    managed: bool,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Check startup prerequisites without starting the server or changing its database.
    #[command(name = SERVER_DOCTOR_COMMAND.name())]
    Doctor(doctor::Arguments),
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let arguments = Arguments::parse();
    if let Some(Command::Doctor(arguments)) = arguments.command {
        let report = pwf_server::doctor::inspect().await;
        println!("{}", arguments.render(&report, Console::from_terminal())?);
        if report.failed() {
            std::process::exit(1);
        }
        return Ok(());
    }
    match pwf_server::run().await {
        Ok(()) => Ok(()),
        Err(pwf_server::RunError::MigrationHistory(reason)) => {
            eprintln!(
                "Error: {reason}\nRun `{DOCTOR_COMMAND}` for migration compatibility and recovery actions."
            );
            // launchd and Task Scheduler retry nonzero exits without a per-code exception.
            let code = if arguments.managed && !cfg!(target_os = "linux") {
                0
            } else {
                78
            };
            std::process::exit(code);
        }
        Err(pwf_server::RunError::Runtime(error)) => Err(error),
    }
}
