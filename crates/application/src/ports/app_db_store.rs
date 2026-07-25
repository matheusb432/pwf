/// Provides access to relational application data.
pub trait AppDbStore: Clone + Send + Sync + 'static {
    /// Returns the relational application-data pool.
    fn pool(&self) -> &sqlx::SqlitePool;
}

#[cfg(test)]
#[derive(Clone)]
pub(crate) struct TestDatabase(sqlx::SqlitePool);

#[cfg(test)]
impl TestDatabase {
    pub(crate) async fn new() -> Self {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE project_sources (
                id INTEGER PRIMARY KEY,
                kind TEXT NOT NULL,
                value TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT '2026-07-26T00:00:00.000Z',
                UNIQUE (kind, value)
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE projects (
                id TEXT PRIMARY KEY,
                project_source_id INTEGER NOT NULL,
                title TEXT NOT NULL UNIQUE,
                tasks_kind TEXT NOT NULL,
                tasks_path TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT '2026-07-26T00:00:00.000Z',
                paused_at TEXT,
                UNIQUE (tasks_kind, tasks_path)
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE VIEW active_projects AS
             SELECT id, project_source_id, title, tasks_kind, tasks_path, created_at
             FROM projects
             WHERE paused_at IS NULL",
        )
        .execute(&pool)
        .await
        .unwrap();
        Self(pool)
    }

    pub(crate) async fn insert_project(
        &self,
        id: &str,
        title: &str,
        source_value: &str,
        tasks_path: &str,
        is_paused: bool,
    ) {
        let source_id =
            sqlx::query("INSERT INTO project_sources (kind, value) VALUES ('directory', ?)")
                .bind(source_value)
                .execute(self.pool())
                .await
                .unwrap()
                .last_insert_rowid();
        let paused_at = is_paused.then_some("2026-07-26T00:00:00.000Z");
        sqlx::query(
            "INSERT INTO projects (
                id,
                project_source_id,
                title,
                tasks_kind,
                tasks_path,
                paused_at
            )
            VALUES (?, ?, ?, 'directory', ?, ?)",
        )
        .bind(id)
        .bind(source_id)
        .bind(title)
        .bind(tasks_path)
        .bind(paused_at)
        .execute(self.pool())
        .await
        .unwrap();
    }
}

#[cfg(test)]
impl AppDbStore for TestDatabase {
    fn pool(&self) -> &sqlx::SqlitePool {
        &self.0
    }
}
