use std::assert_matches;

use pwf_application::project::{get_project, update_project, update_project::UpdateProjectError};
use pwf_models::project::{Project, ProjectSource, ProjectSourceKind, ProjectSourceValue};
use pwf_wire::{
    patch_field::PatchField,
    project::{GetProject, ProjectStatusFilter, UpdateProject},
    set_field::SetField,
};

use crate::support::insert_project;

fn update(project_id: &str, source_value: &str) -> UpdateProject {
    UpdateProject {
        source: PatchField::Set(ProjectSource::new(
            ProjectSourceKind::Directory,
            ProjectSourceValue::try_new(source_value).unwrap(),
        )),
        ..unchanged_update(project_id)
    }
}

fn unchanged_update(project_id: &str) -> UpdateProject {
    UpdateProject {
        id: project_id.parse().unwrap(),
        source: PatchField::NoAction,
        obsidian_vault: PatchField::NoAction,
        snapshot_enabled: SetField::NoAction,
    }
}

async fn read_project(project_id: &str, pool: &sqlx::SqlitePool) -> Project {
    get_project::execute(
        GetProject {
            id: project_id.parse().unwrap(),
            status: ProjectStatusFilter::IncludingPaused,
        },
        &pwf_infra::project_store::SqliteProjectStore::new(pool.clone()),
    )
    .await
    .unwrap()
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn source_update_is_atomic_idempotent_and_reuses_shared_sources(pool: sqlx::SqlitePool) {
    insert_project(&pool, "FOO", "foo", "/work/old", "/tasks/foo", true).await;
    insert_project(&pool, "BAR", "bar", "/work/shared", "/tasks/bar", false).await;

    update_project::execute(
        update("FOO", "/work/shared"),
        &pwf_infra::project_store::SqliteProjectStore::new(pool.clone()),
    )
    .await
    .unwrap();
    update_project::execute(
        update("FOO", "/work/shared"),
        &pwf_infra::project_store::SqliteProjectStore::new(pool.clone()),
    )
    .await
    .unwrap();

    let stored: (String, String, String, String, bool) = sqlx::query_as(
        r"
        SELECT
            projects.id,
            projects.title,
            project_sources.value,
            projects.tasks_path,
            projects.paused_at IS NOT NULL
        FROM projects
        LEFT JOIN project_sources ON project_sources.id = projects.project_source_id
        WHERE projects.id = 'FOO'
        ",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        stored,
        (
            "FOO".to_string(),
            "foo".to_string(),
            "/work/shared".to_string(),
            "/tasks/foo".to_string(),
            true,
        )
    );
    let shared_source_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM project_sources WHERE value = '/work/shared'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(shared_source_count, 1);
    pool.close().await;
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn missing_project_rolls_back_the_candidate_source(pool: sqlx::SqlitePool) {
    let error = update_project::execute(
        update("MISS", "/work/candidate"),
        &pwf_infra::project_store::SqliteProjectStore::new(pool.clone()),
    )
    .await
    .unwrap_err();

    assert_matches!(
        error,
        UpdateProjectError::ProjectNotFound { id } if id.as_ref() == "MISS"
    );
    let source_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM project_sources WHERE value = '/work/candidate'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(source_count, 0);
    pool.close().await;
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn snapshot_updates_preserve_omitted_fields_when_repeated(pool: sqlx::SqlitePool) {
    insert_project(&pool, "FOO", "foo", "/work/foo", "/tasks/foo", true).await;
    let mut expected = read_project("FOO", &pool).await;
    for enabled in [true, true, false, false] {
        update_project::execute(
            UpdateProject {
                snapshot_enabled: SetField::Set(enabled),
                ..unchanged_update("FOO")
            },
            &pwf_infra::project_store::SqliteProjectStore::new(pool.clone()),
        )
        .await
        .unwrap();

        expected.snapshot_enabled = enabled;
        assert_eq!(read_project("FOO", &pool).await, expected);
    }
    pool.close().await;
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn source_clear_preserves_omitted_fields_when_repeated(pool: sqlx::SqlitePool) {
    insert_project(&pool, "FOO", "foo", "/work/foo", "/tasks/foo", true).await;
    update_project::execute(
        UpdateProject {
            obsidian_vault: PatchField::Set("/vault/foo".parse().unwrap()),
            snapshot_enabled: SetField::Set(true),
            ..unchanged_update("FOO")
        },
        &pwf_infra::project_store::SqliteProjectStore::new(pool.clone()),
    )
    .await
    .unwrap();
    let mut expected = read_project("FOO", &pool).await;
    expected.source = None;
    for _ in 0..2 {
        update_project::execute(
            UpdateProject {
                source: PatchField::Clear,
                ..unchanged_update("FOO")
            },
            &pwf_infra::project_store::SqliteProjectStore::new(pool.clone()),
        )
        .await
        .unwrap();

        assert_eq!(read_project("FOO", &pool).await, expected);
    }
    pool.close().await;
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn missing_project_rejects_updates_without_setting_source(pool: sqlx::SqlitePool) {
    insert_project(&pool, "FOO", "foo", "/work/foo", "/tasks/foo", false).await;
    let expected = read_project("FOO", &pool).await;
    for source in [PatchField::NoAction, PatchField::Clear] {
        let error = update_project::execute(
            UpdateProject {
                source,
                obsidian_vault: PatchField::Set("/vault/missing".parse().unwrap()),
                snapshot_enabled: SetField::Set(true),
                ..unchanged_update("MISS")
            },
            &pwf_infra::project_store::SqliteProjectStore::new(pool.clone()),
        )
        .await
        .unwrap_err();

        assert_matches!(
            error,
            UpdateProjectError::ProjectNotFound { id } if id.as_ref() == "MISS"
        );
    }
    assert_eq!(read_project("FOO", &pool).await, expected);
    pool.close().await;
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn concurrent_disjoint_patches_preserve_each_field(pool: sqlx::SqlitePool) {
    insert_project(&pool, "FOO", "foo", "/work/old", "/tasks/foo", false).await;
    let mut expected = read_project("FOO", &pool).await;
    let vault = "/vault/foo"
        .parse::<pwf_models::project::ObsidianVault>()
        .unwrap();
    let projects = pwf_infra::project_store::SqliteProjectStore::new(pool.clone());
    let (source, obsidian_vault, snapshot_enabled) = tokio::join!(
        update_project::execute(update("FOO", "/work/new"), &projects),
        update_project::execute(
            UpdateProject {
                obsidian_vault: PatchField::Set(vault.clone()),
                ..unchanged_update("FOO")
            },
            &projects,
        ),
        update_project::execute(
            UpdateProject {
                snapshot_enabled: SetField::Set(true),
                ..unchanged_update("FOO")
            },
            &projects,
        ),
    );
    source.unwrap();
    obsidian_vault.unwrap();
    snapshot_enabled.unwrap();

    expected.source = Some(ProjectSource::new(
        ProjectSourceKind::Directory,
        ProjectSourceValue::try_new("/work/new").unwrap(),
    ));
    expected.obsidian_vault = Some(vault);
    expected.snapshot_enabled = true;
    assert_eq!(read_project("FOO", &pool).await, expected);
    pool.close().await;
}
