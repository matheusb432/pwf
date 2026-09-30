use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
};

use anyhow::Context as _;
use clap::{Args, Subcommand};

use crate::server::{
    is_registered_server_active, run_server_subcommand, start_registered_server,
    stop_registered_server,
};

#[derive(Args, Debug)]
pub struct Arguments {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    #[command(about = "Writes a consistent snapshot of pwf's database and portable configuration.")]
    Export(ExportArguments),
    #[command(
        about = "Replaces pwf's database with a snapshot, forward-migrating an older snapshot."
    )]
    Import(ImportArguments),
}

#[derive(Args, Debug)]
pub struct ExportArguments {
    #[arg(
        long,
        value_name = "DIR",
        help = "Snapshot directory to create; its parent must exist."
    )]
    to: PathBuf,
}

#[derive(Args, Debug)]
pub struct ImportArguments {
    #[arg(
        long,
        value_name = "DIR",
        help = "Snapshot directory written by `pwf data export`."
    )]
    from: PathBuf,
}

pub async fn run(arguments: Arguments) -> anyhow::Result<String> {
    match arguments.command {
        Command::Export(arguments) => {
            run_server_data("export", "--to", &arguments.to).await?;
            eprintln!("exported pwf data to {}", arguments.to.display());
        }
        Command::Import(arguments) => {
            run_server_data("stage-import", "--from", &arguments.from).await?;
            let was_active = is_registered_server_active().await?;
            if was_active {
                stop_registered_server().await?;
            }
            run_server_data("finish-import", "--from", &arguments.from).await?;
            if was_active {
                start_registered_server()
                    .await
                    .context("pwf data was imported, but the server did not start")?;
            }
            eprintln!("imported pwf data from {}", arguments.from.display());
        }
    }
    Ok(String::new())
}

async fn run_server_data(subcommand: &str, flag: &str, directory: &Path) -> anyhow::Result<()> {
    run_server_subcommand([
        OsStr::new("data"),
        OsStr::new(subcommand),
        OsStr::new(flag),
        directory.as_os_str(),
    ])
    .await?;
    Ok(())
}
