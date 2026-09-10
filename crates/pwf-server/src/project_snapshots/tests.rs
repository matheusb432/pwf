use std::{
    fs,
    io::Write as _,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::Context as _;
use pwf_infra::user_settings::TomlSettingsStore;
use pwf_models::project::HomeDirectory;
use tokio::{sync::watch, task::JoinHandle};

use crate::AppState;

const REFRESH_INTERVAL_TEST: Duration = Duration::from_millis(20);
const OBSERVATION_TIMEOUT: Duration = Duration::from_secs(3);
const POLL_INTERVAL: Duration = Duration::from_millis(5);
const PROJECT_SNAPSHOT_FILE_NAME: &str = "pwf-index.md";

#[tokio::test]
async fn startup_skips_disabled_projects_and_observes_opt_in_and_paused_projects()
-> anyhow::Result<()> {
    let (_root, state) = state().await?;
    let disabled = register_project(&state, "OFF").await?;
    let paused = register_project(&state, "PAU").await?;
    write_task(&disabled, "OFF-0001", "active")?;
    write_task(&paused, "PAU-0001", "active")?;
    sqlx::query(
        "UPDATE projects SET snapshot_enabled = 1, paused_at = '2026-09-10T00:00:00Z' WHERE id = 'PAU'",
    )
    .execute(&state.pool)
    .await?;
    let (shutdown, receiver) = watch::channel(false);
    let worker = tokio::spawn(super::run(state.clone(), REFRESH_INTERVAL_TEST, receiver));

    await_snapshot(&paused.join(PROJECT_SNAPSHOT_FILE_NAME), |source| {
        source.contains("[[PAU-0001]]")
    })
    .await?;
    let disabled_snapshot = disabled.join(PROJECT_SNAPSHOT_FILE_NAME);
    assert!(!disabled_snapshot.exists());

    sqlx::query("UPDATE projects SET snapshot_enabled = 1 WHERE id = 'OFF'")
        .execute(&state.pool)
        .await?;
    await_snapshot(&disabled_snapshot, |source| source.contains("[[OFF-0001]]")).await?;

    let previous_snapshot = fs::read(&disabled_snapshot)?;
    sqlx::query("UPDATE projects SET snapshot_enabled = 0 WHERE id = 'OFF'")
        .execute(&state.pool)
        .await?;
    let paused_snapshot = paused.join(PROJECT_SNAPSHOT_FILE_NAME);
    write_task(&paused, "PAU-0002", "active")?;
    await_snapshot(&paused_snapshot, |source| source.contains("[[PAU-0002]]")).await?;
    assert_eq!(fs::read(&disabled_snapshot)?, previous_snapshot);

    write_task(&disabled, "OFF-0001", "done")?;
    write_task(&paused, "PAU-0002", "done")?;
    await_snapshot(&paused_snapshot, |source| {
        source.contains("[x] [[PAU-0002]]")
    })
    .await?;
    assert_eq!(fs::read(&disabled_snapshot)?, previous_snapshot);

    stop_worker(shutdown, worker).await?;
    state.pool.close().await;
    Ok(())
}

#[tokio::test]
async fn periodic_refresh_observes_external_task_and_note_changes_until_shutdown()
-> anyhow::Result<()> {
    let (_root, state) = state().await?;
    let directory = register_project(&state, "EXT").await?;
    write_task(&directory, "EXT-0001", "active")?;
    replace_file(&directory.join("EXT-NOTE-0001.md"), "# Original note\n")?;
    sqlx::query("UPDATE projects SET snapshot_enabled = 1")
        .execute(&state.pool)
        .await?;
    let (shutdown, receiver) = watch::channel(false);
    let worker = tokio::spawn(super::run(state.clone(), REFRESH_INTERVAL_TEST, receiver));
    let snapshot = directory.join(PROJECT_SNAPSHOT_FILE_NAME);
    await_snapshot(&snapshot, |source| {
        source.contains("[ ] [[EXT-0001]]") && source.contains("[[EXT-NOTE-0001]]")
    })
    .await?;

    write_task(&directory, "EXT-0001", "done")?;
    write_task(&directory, "EXT-0002", "active")?;
    fs::remove_file(directory.join("EXT-NOTE-0001.md"))?;
    replace_file(&directory.join("EXT-NOTE-0002.md"), "# Replacement note\n")?;
    await_snapshot(&snapshot, |source| {
        source.contains("[x] [[EXT-0001]]")
            && source.contains("[[EXT-0002]]")
            && source.contains("[[EXT-NOTE-0002]]")
            && !source.contains("EXT-NOTE-0001")
    })
    .await?;

    stop_worker(shutdown, worker).await?;
    let stopped_snapshot = fs::read(&snapshot)?;
    write_task(&directory, "EXT-0003", "active")?;
    fs::remove_file(directory.join("EXT-NOTE-0002.md"))?;
    assert!(
        tokio::time::timeout(
            REFRESH_INTERVAL_TEST * 5,
            await_snapshot(&snapshot, |source| source.as_bytes() != stopped_snapshot),
        )
        .await
        .is_err(),
        "snapshot changed after the worker joined"
    );
    assert_eq!(fs::read(&snapshot)?, stopped_snapshot);
    state.pool.close().await;
    Ok(())
}

#[tokio::test]
async fn project_failure_preserves_previous_snapshot_and_other_projects_keep_refreshing()
-> anyhow::Result<()> {
    let (_root, state) = state().await?;
    let broken = register_project(&state, "BAD").await?;
    let healthy = register_project(&state, "GOOD").await?;
    write_task(&broken, "BAD-0001", "active")?;
    write_task(&healthy, "GOOD-0001", "active")?;
    sqlx::query("UPDATE projects SET snapshot_enabled = 1")
        .execute(&state.pool)
        .await?;
    let (shutdown, receiver) = watch::channel(false);
    let worker = tokio::spawn(super::run(state.clone(), REFRESH_INTERVAL_TEST, receiver));
    let broken_snapshot = broken.join(PROJECT_SNAPSHOT_FILE_NAME);
    let healthy_snapshot = healthy.join(PROJECT_SNAPSHOT_FILE_NAME);
    await_snapshot(&healthy_snapshot, |source| source.contains("[[GOOD-0001]]")).await?;
    let previous_snapshot = fs::read(&broken_snapshot)?;

    write_task(&broken, "BAD-0001", "invalid")?;
    write_task(&healthy, "GOOD-0001", "done")?;
    await_snapshot(&healthy_snapshot, |source| {
        source.contains("[x] [[GOOD-0001]]")
    })
    .await?;
    assert_eq!(fs::read(&broken_snapshot)?, previous_snapshot);

    write_task(&healthy, "GOOD-0002", "active")?;
    await_snapshot(&healthy_snapshot, |source| source.contains("[[GOOD-0002]]")).await?;
    assert_eq!(fs::read(&broken_snapshot)?, previous_snapshot);

    write_task(&broken, "BAD-0001", "done")?;
    await_snapshot(&broken_snapshot, |source| {
        source.contains("[x] [[BAD-0001]]")
    })
    .await?;
    let previous_snapshot = fs::read(&broken_snapshot)?;
    let note = broken.join("BAD-NOTE-0001.md");
    replace_file(&note, "---\ntype: note\n---\n")?;
    write_task(&healthy, "GOOD-0002", "done")?;
    await_snapshot(&healthy_snapshot, |source| {
        source.contains("[x] [[GOOD-0002]]")
    })
    .await?;
    assert_eq!(fs::read(&broken_snapshot)?, previous_snapshot);

    write_task(&healthy, "GOOD-0003", "active")?;
    await_snapshot(&healthy_snapshot, |source| source.contains("[[GOOD-0003]]")).await?;
    assert_eq!(fs::read(&broken_snapshot)?, previous_snapshot);

    replace_file(&note, "# Recovered project note\n")?;
    await_snapshot(&broken_snapshot, |source| {
        source.contains("[[BAD-NOTE-0001]]") && source.contains("[x] [[BAD-0001]]")
    })
    .await?;
    stop_worker(shutdown, worker).await?;
    state.pool.close().await;
    Ok(())
}

async fn state() -> anyhow::Result<(tempfile::TempDir, AppState)> {
    let root = tempfile::tempdir()?;
    let database_path = root.path().join("pwf.sqlite3");
    pwf_migrator::run(&database_path).await?;
    let pool = pwf_infra::database::build_pool(&database_path).await?;
    let state = AppState::new(
        pool,
        HomeDirectory::new(root.path().to_path_buf()),
        TomlSettingsStore::new(Some(root.path().join("config.toml"))),
    );
    Ok((root, state))
}

async fn register_project(state: &AppState, id: &str) -> anyhow::Result<PathBuf> {
    let directory = state.home.as_path().join(id);
    fs::create_dir_all(&directory)?;
    sqlx::query(
        "INSERT INTO projects (id, title, tasks_kind, tasks_path) VALUES (?, ?, 'directory', ?)",
    )
    .bind(id)
    .bind(id)
    .bind(directory.to_str().context("project path is not Unicode")?)
    .execute(&state.pool)
    .await?;
    Ok(directory)
}

fn write_task(directory: &Path, id: &str, status: &str) -> anyhow::Result<()> {
    replace_file(
        &directory.join(format!("{id}.md")),
        &format!("---\nid: {id}\nstatus: {status}\ntitle: Snapshot task\n---\n\nTask body.\n"),
    )
}

fn replace_file(path: &Path, source: &str) -> anyhow::Result<()> {
    let mut temporary =
        tempfile::NamedTempFile::new_in(path.parent().context("file has no parent directory")?)?;
    temporary.write_all(source.as_bytes())?;
    temporary.persist(path)?;
    Ok(())
}

async fn await_snapshot(path: &Path, predicate: impl Fn(&str) -> bool) -> anyhow::Result<()> {
    tokio::time::timeout(OBSERVATION_TIMEOUT, async {
        loop {
            if fs::read_to_string(path).is_ok_and(|source| predicate(&source)) {
                return;
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    })
    .await
    .with_context(|| format!("waiting for snapshot {}", path.display()))
}

async fn stop_worker(shutdown: watch::Sender<bool>, worker: JoinHandle<()>) -> anyhow::Result<()> {
    shutdown.send(true)?;
    tokio::time::timeout(OBSERVATION_TIMEOUT, worker)
        .await
        .context("waiting for snapshot worker shutdown")??;
    Ok(())
}
