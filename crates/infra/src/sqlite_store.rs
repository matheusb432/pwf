use pwf_application::AppDbStore;
use sqlx::SqlitePool;

/// Stores relational application data in `SQLite`.
#[derive(Debug, Clone)]
pub struct SqliteStore(SqlitePool);

impl SqliteStore {
    #[must_use]
    pub fn new(pool: SqlitePool) -> Self {
        Self(pool)
    }
}

impl AppDbStore for SqliteStore {
    fn pool(&self) -> &SqlitePool {
        &self.0
    }
}
