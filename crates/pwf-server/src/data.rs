use std::{
    fs, io,
    path::{Path, PathBuf},
};

use anyhow::{Context as _, ensure};
use pwf_infra::database::snapshot;

const SNAPSHOT_FORMAT_VERSION: u32 = 1;
const SNAPSHOT_APPLICATION: &str = "pwf";
const SNAPSHOT_MANIFEST_FILE_NAME: &str = "manifest.json";
const SNAPSHOT_DATABASE_FILE_NAME: &str = "pwf.sqlite3";
const SNAPSHOT_SETTINGS_FILE_NAMES: [&str; 2] = ["config.toml", "titles.toml"];
const STAGED_DATABASE_SUFFIX: &str = ".import";

pub struct StagedSnapshot {
    pub snapshot_schema_version: i64,
    pub schema_version: i64,
}

pub async fn export(snapshot_directory: &Path) -> anyhow::Result<i64> {
    fs::create_dir(snapshot_directory).with_context(|| {
        format!(
            "creating snapshot directory {}",
            snapshot_directory.display()
        )
    })?;
    let exported = write_snapshot(snapshot_directory).await;
    if exported.is_err() {
        let _ = fs::remove_dir_all(snapshot_directory);
    }
    exported
}

pub async fn stage_import(snapshot_directory: &Path) -> anyhow::Result<StagedSnapshot> {
    read_manifest(snapshot_directory)?;
    let database_path = pwf_infra::database::database_path()?;
    let staged_path = staged_database_path(&database_path);
    remove_file_if_present(&staged_path)?;
    match snapshot::stage_snapshot(
        &snapshot_directory.join(SNAPSHOT_DATABASE_FILE_NAME),
        &staged_path,
    )
    .await
    {
        Ok((snapshot_schema_version, schema_version)) => Ok(StagedSnapshot {
            snapshot_schema_version,
            schema_version,
        }),
        Err(error) => {
            let _ = fs::remove_file(&staged_path);
            Err(error)
        }
    }
}

pub fn finish_import(snapshot_directory: &Path) -> anyhow::Result<()> {
    let database_path = pwf_infra::database::database_path()?;
    let staged_path = staged_database_path(&database_path);
    ensure!(
        staged_path.is_file(),
        "no staged pwf import at {}",
        staged_path.display()
    );
    snapshot::replace_database_file(&staged_path, &database_path)?;
    restore_user_settings(snapshot_directory)
}

async fn write_snapshot(snapshot_directory: &Path) -> anyhow::Result<i64> {
    let database_path = pwf_infra::database::database_path()?;
    let schema_version = snapshot::export_snapshot(
        &database_path,
        &snapshot_directory.join(SNAPSHOT_DATABASE_FILE_NAME),
    )
    .await?;
    if let Some(config_path) = pwf_infra::user_settings::config_path() {
        for filename in SNAPSHOT_SETTINGS_FILE_NAMES {
            let path = config_path.with_file_name(filename);
            if path.is_file() {
                fs::copy(&path, snapshot_directory.join(filename))
                    .with_context(|| format!("copying user settings {}", path.display()))?;
            }
        }
    }
    let manifest = serde_json::json!({
        "format_version": SNAPSHOT_FORMAT_VERSION,
        "application": SNAPSHOT_APPLICATION,
        "schema_version": schema_version,
    });
    fs::write(
        snapshot_directory.join(SNAPSHOT_MANIFEST_FILE_NAME),
        serde_json::to_string_pretty(&manifest)? + "\n",
    )
    .context("writing snapshot manifest")?;
    Ok(schema_version)
}

fn read_manifest(snapshot_directory: &Path) -> anyhow::Result<()> {
    let path = snapshot_directory.join(SNAPSHOT_MANIFEST_FILE_NAME);
    let text = fs::read_to_string(&path)
        .with_context(|| format!("reading snapshot manifest {}", path.display()))?;
    let manifest: serde_json::Value = serde_json::from_str(&text)
        .with_context(|| format!("parsing snapshot manifest {}", path.display()))?;
    let application = manifest["application"].as_str().unwrap_or_default();
    ensure!(
        application == SNAPSHOT_APPLICATION,
        "snapshot belongs to `{application}`, not `{SNAPSHOT_APPLICATION}`"
    );
    ensure!(
        manifest["format_version"].as_u64() == Some(u64::from(SNAPSHOT_FORMAT_VERSION)),
        "snapshot format version {} is unsupported; expected {SNAPSHOT_FORMAT_VERSION}",
        manifest["format_version"]
    );
    ensure!(
        manifest["schema_version"].is_i64(),
        "snapshot manifest {} has no integer schema_version",
        path.display()
    );
    Ok(())
}

fn restore_user_settings(snapshot_directory: &Path) -> anyhow::Result<()> {
    for filename in SNAPSHOT_SETTINGS_FILE_NAMES {
        let snapshot_path = snapshot_directory.join(filename);
        if !snapshot_path.is_file() {
            continue;
        }
        let config_path = pwf_infra::user_settings::config_path()
            .context("resolving the user settings path to restore configuration")?
            .with_file_name(filename);
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("creating user settings directory {}", parent.display())
            })?;
        }
        fs::copy(&snapshot_path, &config_path)
            .with_context(|| format!("writing user settings {}", config_path.display()))?;
    }
    Ok(())
}

fn staged_database_path(database_path: &Path) -> PathBuf {
    let mut staged = database_path.as_os_str().to_os_string();
    staged.push(STAGED_DATABASE_SUFFIX);
    staged.into()
}

fn remove_file_if_present(path: &Path) -> anyhow::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => {
            Err(error).with_context(|| format!("removing stale staged import {}", path.display()))
        }
    }
}
