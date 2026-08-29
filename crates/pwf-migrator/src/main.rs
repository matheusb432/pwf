//! Dedicated one-shot persistence migration process.

use anyhow::{Context as _, bail};
use clap::Parser as _;
use pwf_application::{
    ports::task_metadata_migration::{TaskMetadataMigrationMode, TaskMetadataMigrationReport},
    task::migrate_task_metadata,
};
use pwf_infra::obsidian::ObsidianStore;
use pwf_models::project::HomeDirectory;

#[derive(Debug, clap::Parser)]
#[command(about = "Apply or check PWF persistence migrations")]
struct Args {
    /// Report pending migrations without changing the database or task vaults.
    #[arg(long)]
    check: bool,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    run(Args::parse()).await
}

async fn run(args: Args) -> anyhow::Result<()> {
    let path = pwf_infra::database::database_path()?;
    let pool = if args.check {
        pwf_infra::database::build_read_only_pool(&path).await?
    } else {
        pwf_infra::database::build_migration_pool(&path).await?
    };
    if args.check {
        pwf_infra::database::check_database_ready(&pool).await?;
    } else {
        pwf_infra::database::migrate_database(&pool).await?;
    }
    println!("Database migrations are current: `{}`", path.display());

    let home = directories::BaseDirs::new()
        .map(|directories| HomeDirectory::new(directories.home_dir().to_path_buf()))
        .context("resolving the home directory for managed projects")?;
    let store = ObsidianStore::new(home);
    let mode = if args.check {
        TaskMetadataMigrationMode::Check
    } else {
        TaskMetadataMigrationMode::Apply
    };
    let report = migrate_task_metadata::execute(mode, &store, &pool).await?;
    pool.close().await;
    print_report(&report);
    if report.has_issues() {
        for issue in &report.issues {
            let location = issue.path.as_ref().map_or_else(
                || issue.project.to_string(),
                |path| path.display().to_string(),
            );
            eprintln!("{location}: {}", issue.message);
        }
        bail!(
            "task metadata migration could not process {} item(s)",
            report.issues.len()
        );
    }
    if args.check && report.has_changes() {
        bail!("task metadata migration is pending; run `just migrate`");
    }
    println!("Task metadata migrations are current.");
    Ok(())
}

fn print_report(report: &TaskMetadataMigrationReport) {
    println!(
        "Task metadata: {} project(s), {} task file(s), {} task file change(s), {} created_at migration(s), {} completed_at migration(s), {} mtime fallback(s), {} index entry change(s).",
        report.project_count,
        report.task_file_count,
        report.task_file_changed_count,
        report.created_at_migrated_count,
        report.completed_at_migrated_count,
        report.completed_at_from_modified_count,
        report.index_entry_changed_count,
    );
}

#[cfg(test)]
mod tests {
    use clap::Parser as _;

    use super::Args;

    #[test]
    fn check_flag_selects_read_only_migration() {
        assert!(
            Args::try_parse_from(["pwf-migrator", "--check"])
                .unwrap()
                .check
        );
        assert!(!Args::try_parse_from(["pwf-migrator"]).unwrap().check);
    }
}
