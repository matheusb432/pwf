use std::{io::Read as _, path::Path};

use anyhow::{Context as _, Result, ensure};

use crate::draft_file;

const DRAFT_BYTES_MAX: u64 = 4 * 1024 * 1024;

pub(super) struct EditedInput {
    pub text: String,
    pub error: Option<String>,
}

fn editor_command() -> Result<Vec<String>> {
    let editor = ["VISUAL", "EDITOR"]
        .into_iter()
        .find_map(|key| {
            std::env::var(key)
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
        .unwrap_or_else(|| "vi".into());
    let command = shell_words::split(&editor)
        .context("Cannot parse VISUAL/EDITOR; use an executable followed by optional arguments.")?;
    ensure!(!command.is_empty(), "The editor command is empty.");
    Ok(command)
}

pub(super) async fn edit_file(path: &Path) -> Result<()> {
    let command = editor_command()?;
    let status = tokio::process::Command::new(&command[0])
        .args(&command[1..])
        .arg(path)
        .kill_on_drop(true)
        .status()
        .await
        .with_context(|| format!("Cannot start editor {}", command[0]))?;
    ensure!(
        status.success(),
        "Editor exited with {status}. Saved Markdown remains at {}.",
        path.display()
    );
    Ok(())
}

pub(super) async fn edit_input(text: String) -> Result<EditedInput> {
    let source = text.clone();
    let file = tokio::task::spawn_blocking(move || draft_file::create(&source)).await??;
    let error = edit_file(file.path())
        .await
        .err()
        .map(|error| format!("{error:#}"));
    tokio::task::spawn_blocking(move || {
        let read = || -> Result<String> {
            let mut contents = String::new();
            std::fs::File::open(file.path())?
                .take(DRAFT_BYTES_MAX + 1)
                .read_to_string(&mut contents)?;
            ensure!(
                contents.len() as u64 <= DRAFT_BYTES_MAX,
                "Edited draft exceeds 4 MiB."
            );
            Ok(contents)
        };
        match read() {
            Ok(contents) => Ok(EditedInput {
                text: contents,
                error,
            }),
            Err(read_error) => {
                let (_, path) = file
                    .keep()
                    .context("Cannot retain the editor draft file.")?;
                Ok(EditedInput {
                    text,
                    error: Some(format!(
                        "{read_error:#} Recover editor input from {}.",
                        path.display()
                    )),
                })
            }
        }
    })
    .await?
}
