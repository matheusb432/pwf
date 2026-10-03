//! Interactive frontend; task and note operations use the resident server.

mod app;
mod backend;
mod browser;
mod draft;
mod draft_file;
mod editor;
mod references;
mod runtime;
mod text_input;
mod view;

#[cfg(test)]
mod test_support;

use anyhow::Result;
use pwf_models::project::ProjectId;

#[derive(Debug, clap::Args)]
pub struct Arguments {
    /// Initially show this project's records; otherwise infer it from the working directory.
    /// Outside registered task and session directories, show every active project.
    pub project: Option<ProjectId>,
}

/// Runs until the user quits, restoring the terminal on normal and error exits.
/// The binary edge must verify that stdin and stdout are interactive terminals first.
pub async fn run(arguments: Arguments) -> Result<()> {
    runtime::run(arguments.project).await
}
