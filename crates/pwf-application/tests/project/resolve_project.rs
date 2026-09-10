use pwf_application::project::resolve_project::{self, ResolveProjectError};
use pwf_wire::project::{ProjectStatusFilter, ResolveProject};

use crate::support::{insert_project, insert_unrelated_invalid_project};

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn title_precedes_id_without_loading_unrelated_projects(pool: sqlx::SqlitePool) {
    insert_project(&pool, "ALT", "other", "/work/alt", "/tasks/alt", false).await;
    insert_project(&pool, "FOO", "alt", "/work/foo", "/tasks/foo", false).await;
    insert_unrelated_invalid_project(&pool).await;
    for selector in ["alt", "ALT", " Alt ", "foo", "FOO"] {
        let id = resolve_project::execute(
            ResolveProject {
                selector: selector.parse().unwrap(),
                status: ProjectStatusFilter::ActiveOnly,
            },
            &pool,
        )
        .await
        .unwrap();
        assert_eq!(id.as_ref(), "FOO");
    }
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn paused_title_only_precedes_id_when_included(pool: sqlx::SqlitePool) {
    insert_project(&pool, "ALT", "other", "/work/alt", "/tasks/alt", false).await;
    insert_project(&pool, "FOO", "alt", "/work/foo", "/tasks/foo", true).await;
    for (status, expected) in [
        (ProjectStatusFilter::ActiveOnly, "ALT"),
        (ProjectStatusFilter::IncludingPaused, "FOO"),
    ] {
        let id = resolve_project::execute(
            ResolveProject {
                selector: "alt".parse().unwrap(),
                status,
            },
            &pool,
        )
        .await
        .unwrap();
        assert_eq!(id.as_ref(), expected);
    }
    let error = resolve_project::execute(
        ResolveProject {
            selector: "foo".parse().unwrap(),
            status: ProjectStatusFilter::ActiveOnly,
        },
        &pool,
    )
    .await
    .unwrap_err();
    assert!(matches!(error, ResolveProjectError::ProjectNotFound { .. }));
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn names_use_exact_ascii_case_insensitive_matching(pool: sqlx::SqlitePool) {
    insert_project(&pool, "FOO", "foo-bar", "/work/foo", "/tasks/foo", false).await;
    insert_project(&pool, "AUX", "éclair", "/work/aux", "/tasks/aux", false).await;
    for (selector, expected) in [
        ("FOO-BAR", Some("FOO")),
        ("éCLAIR", Some("AUX")),
        ("ÉCLAIR", None),
        ("foo%", None),
    ] {
        let result = resolve_project::execute(
            ResolveProject {
                selector: selector.parse().unwrap(),
                status: ProjectStatusFilter::ActiveOnly,
            },
            &pool,
        )
        .await;
        match expected {
            Some(expected) => assert_eq!(result.unwrap().as_ref(), expected),
            None => assert!(matches!(
                result,
                Err(ResolveProjectError::ProjectNotFound { .. })
            )),
        }
    }
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn unknown_selector_lists_only_eligible_titles_in_order(pool: sqlx::SqlitePool) {
    insert_project(&pool, "ZZ", "zeta", "/work/zz", "/tasks/zz", false).await;
    insert_project(&pool, "AA", "alpha", "/work/aa", "/tasks/aa", false).await;
    insert_project(&pool, "PP", "paused", "/work/pp", "/tasks/pp", true).await;
    let error = resolve_project::execute(
        ResolveProject {
            selector: "missing".parse().unwrap(),
            status: ProjectStatusFilter::ActiveOnly,
        },
        &pool,
    )
    .await
    .unwrap_err();
    assert!(
        matches!(error, ResolveProjectError::ProjectNotFound { selector, known }
        if selector.as_ref() == "missing" && known == "alpha, zeta")
    );
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn empty_registry_returns_not_found(pool: sqlx::SqlitePool) {
    let error = resolve_project::execute(
        ResolveProject {
            selector: "missing".parse().unwrap(),
            status: ProjectStatusFilter::ActiveOnly,
        },
        &pool,
    )
    .await
    .unwrap_err();
    assert!(
        matches!(error, ResolveProjectError::ProjectNotFound { known, .. } if known.is_empty())
    );
}
