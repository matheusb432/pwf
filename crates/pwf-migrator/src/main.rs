//! Dedicated one-shot database migration process.

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let path = pwf_infra::database::database_path()?;
    let pool = pwf_infra::database::build_migration_pool(&path).await?;
    pwf_infra::database::migrate_database(&pool).await?;
    println!("Database migrations are current: `{}`", path.display());
    Ok(())
}
