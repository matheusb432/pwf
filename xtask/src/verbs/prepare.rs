use std::{path::Path, process::Command, time::Duration};

use anyhow::{Context, Result};

use crate::{paths, process, sqlite_url};

const DATABASE_SETUP_DEADLINE: Duration = Duration::from_secs(30);
const QUERY_PREPARE_DEADLINE: Duration = Duration::from_mins(5);

pub(crate) fn run(check: bool) -> Result<()> {
    exec(&paths::repo_root(), check)
}

fn exec(root: &Path, check: bool) -> Result<()> {
    let database_directory = tempfile::tempdir()?;
    let database_path = database_directory
        .path()
        .canonicalize()
        .context("resolving SQLx preparation database directory")?
        .join("prepare.db");
    let database_url = sqlite_url::from_path(&database_path);
    let environment = query_environment(&database_url);

    run_sqlx(
        root,
        "create SQLx preparation database",
        &database_setup_arguments(),
        &environment,
        DATABASE_SETUP_DEADLINE,
    )?;
    run_sqlx(
        root,
        "prepare checked SQLx queries",
        &prepare_arguments(check),
        &environment,
        QUERY_PREPARE_DEADLINE,
    )
}

fn run_sqlx(
    root: &Path,
    label: &str,
    arguments: &[&str],
    environment: &[(&str, &str)],
    deadline: Duration,
) -> Result<()> {
    let mut command = Command::new("sqlx");
    command
        .args(arguments)
        .current_dir(root)
        .envs(environment.iter().copied());
    process::run_bounded(label, command, deadline)
}

fn prepare_arguments(check: bool) -> Vec<&'static str> {
    let mut arguments = vec!["prepare"];
    if check {
        arguments.push("--check");
    }
    arguments.extend([
        "--workspace",
        "--no-dotenv",
        "--",
        "--package",
        "pwf-application",
    ]);
    arguments
}

fn database_setup_arguments() -> [&'static str; 4] {
    [
        "database",
        "setup",
        "--source",
        "crates/pwf-infra/migrations",
    ]
}

fn query_environment(database_url: &str) -> [(&'static str, &str); 2] {
    [("DATABASE_URL", database_url), ("SQLX_OFFLINE", "false")]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepare_check_changes_only_the_sqlx_check_flag() {
        assert_eq!(
            prepare_arguments(false),
            [
                "prepare",
                "--workspace",
                "--no-dotenv",
                "--",
                "--package",
                "pwf-application",
            ]
        );
        assert_eq!(
            prepare_arguments(true),
            [
                "prepare",
                "--check",
                "--workspace",
                "--no-dotenv",
                "--",
                "--package",
                "pwf-application",
            ]
        );
    }

    #[test]
    fn database_setup_uses_the_infra_migrations() {
        assert_eq!(
            database_setup_arguments(),
            [
                "database",
                "setup",
                "--source",
                "crates/pwf-infra/migrations"
            ]
        );
    }

    #[test]
    fn setup_and_prepare_share_online_query_environment() {
        assert_eq!(
            query_environment("sqlite:///tmp/prepare.db"),
            [
                ("DATABASE_URL", "sqlite:///tmp/prepare.db"),
                ("SQLX_OFFLINE", "false"),
            ]
        );
    }
}
