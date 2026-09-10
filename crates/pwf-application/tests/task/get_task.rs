use pwf_application::task::{get_task, get_task_record};
use pwf_models::task::{EffortTier, PriorityTier, TaskTags};
use pwf_wire::task::{RawTaskTags, StoredBlockedBy};

use crate::support::{InMemoryStore, insert_project, task_record};

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn get_task_returns_parsed_values_without_changing_the_body(pool: sqlx::SqlitePool) {
    insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
    let mut record = task_record("FOO-0001");
    record.title = "Typed task".to_string();
    record.body = "\n  exact authored body  \n".to_string();
    record.tags = Some(RawTaskTags::new("[Rust, SQLite]"));
    record.effort = Some(" high ".to_string());
    record.priority = Some("highest".to_string());
    record.commits = Some("'a..b, a..b, c..d'".to_string());
    record.created_at = Some("2026-07-26T12:34:56Z".parse().unwrap());
    let store = InMemoryStore::default().with_project("foo", vec![record.clone()]);
    let task = get_task::execute(&record.id, &store, &pool).await.unwrap();
    assert_eq!(task.title.as_ref(), "typed task");
    assert_eq!(task.prompt.as_ref(), record.body);
    assert_eq!(
        task.tags.as_ref(),
        Some(&TaskTags::parse_frontmatter("[rust, sqlite]").unwrap())
    );
    assert_eq!(task.effort, Some(EffortTier::High));
    assert_eq!(task.priority, Some(PriorityTier::Highest));
    assert_eq!(task.commits.as_ref().unwrap().as_ref(), "a..b, c..d");
    assert_eq!(task.created_at, record.created_at);
    assert_eq!(task.revision, record.revision);
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn get_task_rejects_malformed_metadata_while_raw_lookup_preserves_it(pool: sqlx::SqlitePool) {
    insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
    for field in [
        "title",
        "tags",
        "effort",
        "priority",
        "commits",
        "blocked_by",
    ] {
        let mut record = task_record("FOO-0001");
        match field {
            "title" => record.title = "x".repeat(201),
            "tags" => record.tags = Some(RawTaskTags::new("[]")),
            "effort" => record.effort = Some("extreme".to_string()),
            "priority" => record.priority = Some("urgent".to_string()),
            "commits" => record.commits = Some("''".to_string()),
            "blocked_by" => {
                record.blocked_by = StoredBlockedBy::Malformed {
                    raw: "bad links".to_string(),
                    reason: "expected a sequence".to_string(),
                }
            }
            _ => {}
        }
        let store = InMemoryStore::default().with_project("foo", vec![record.clone()]);
        assert_eq!(
            get_task_record::execute(&record.id, &store, &pool)
                .await
                .unwrap(),
            record
        );
        let error = get_task::execute(&record.id, &store, &pool)
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains(field), "{error}");
        assert!(
            error.contains(record.locator.to_string().as_str()),
            "{error}"
        );
    }
}
