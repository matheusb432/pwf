use std::path::PathBuf;

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
    #[command(
        hide = true,
        about = "Internal one-shot snapshot operations for `pwf data export` and `pwf data import`."
    )]
    Data {
        #[command(subcommand)]
        command: DataCommand,
    },
}

#[derive(Subcommand)]
enum DataCommand {
    #[command(
        about = "Writes a snapshot of the live database and user settings into a new directory."
    )]
    Export {
        #[arg(long)]
        to: PathBuf,
    },
    #[command(
        about = "Copies, forward-migrates, and integrity-checks a snapshot into a staging file."
    )]
    StageImport {
        #[arg(long)]
        from: PathBuf,
    },
    #[command(
        about = "Moves the staged database into place and restores the snapshot's user settings. The server must already be stopped."
    )]
    FinishImport {
        #[arg(long)]
        from: PathBuf,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let arguments = Arguments::parse();
    match arguments.command {
        Some(Command::Doctor(doctor_arguments)) => {
            let report = pwf_server::doctor::inspect().await;
            println!(
                "{}",
                doctor_arguments.render(&report, Console::from_terminal())?
            );
            if report.failed() {
                std::process::exit(1);
            }
            Ok(())
        }
        Some(Command::Data { command }) => {
            if let Err(error) = run_data_command(command).await {
                eprintln!("Error: {error:#}");
                std::process::exit(1);
            }
            Ok(())
        }
        None => match pwf_server::run().await {
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
        },
    }
}

async fn run_data_command(command: DataCommand) -> anyhow::Result<()> {
    match command {
        DataCommand::Export { to } => {
            let schema_version = pwf_server::data::export(&to).await?;
            eprintln!(
                "exported pwf data at schema version {schema_version} to {}",
                to.display()
            );
        }
        DataCommand::StageImport { from } => {
            let staged = pwf_server::data::stage_import(&from).await?;
            eprintln!(
                "staged pwf data from {} and migrated schema version {} to {}",
                from.display(),
                staged.snapshot_schema_version,
                staged.schema_version
            );
        }
        DataCommand::FinishImport { from } => {
            pwf_server::data::finish_import(&from)?;
        }
    }
    Ok(())
}
