use std::sync::Arc;

use pwf_application::{
    ports::task_marker_section_store::TaskMarkerSectionStore,
    task::{TaskMarkerSectionRow, TaskMarkerSections, TaskMarkerSectionsError},
};
use tokio::sync::OnceCell;

#[derive(Clone)]
pub struct SqliteTaskMarkerSectionStore {
    pool: sqlx::SqlitePool,
    sections: Arc<OnceCell<Arc<TaskMarkerSections>>>,
}

impl SqliteTaskMarkerSectionStore {
    #[must_use]
    pub fn new(pool: sqlx::SqlitePool) -> Self {
        Self {
            pool,
            sections: Arc::default(),
        }
    }
}

impl TaskMarkerSectionStore for SqliteTaskMarkerSectionStore {
    async fn get_task_marker_sections(
        &self,
    ) -> Result<Arc<TaskMarkerSections>, TaskMarkerSectionsError> {
        self.sections
            .get_or_try_init(|| async {
                let rows = sqlx::query!(
                    r#"
            SELECT
                section AS "section!",
                marker AS "marker!",
                header AS "header!"
            FROM task_marker_sections
            ORDER BY CASE section
                WHEN 'goals' THEN 0
                WHEN 'context' THEN 1
                WHEN 'constraints' THEN 2
                WHEN 'done_when' THEN 3
                ELSE 4
            END
            "#,
                )
                .fetch_all(&self.pool)
                .await
                .map_err(|error| TaskMarkerSectionsError::Database(error.into()))?;

                TaskMarkerSections::try_from_rows(
                    rows.into_iter()
                        .map(|row| TaskMarkerSectionRow {
                            section: row.section,
                            marker: row.marker,
                            header: row.header,
                        })
                        .collect(),
                )
                .map(Arc::new)
            })
            .await
            .cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[sqlx::test]
    async fn successful_configuration_is_shared_without_another_database_read(
        pool: sqlx::SqlitePool,
    ) {
        let store = SqliteTaskMarkerSectionStore::new(pool.clone());
        let first = store.get_task_marker_sections().await.unwrap();
        pool.close().await;
        let second = store.clone().get_task_marker_sections().await.unwrap();
        assert!(Arc::ptr_eq(&first, &second));
    }

    #[sqlx::test]
    async fn failed_configuration_load_can_be_retried(pool: sqlx::SqlitePool) {
        sqlx::query("DELETE FROM task_marker_sections WHERE section = 'goals'")
            .execute(&pool)
            .await
            .unwrap();
        let store = SqliteTaskMarkerSectionStore::new(pool.clone());
        assert!(matches!(
            store.get_task_marker_sections().await,
            Err(TaskMarkerSectionsError::InvalidSectionSet { .. })
        ));
        sqlx::query(
            "INSERT INTO task_marker_sections (section, marker, header) VALUES ('goals', '/g', 'Goals')",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert!(store.get_task_marker_sections().await.is_ok());
    }
}
