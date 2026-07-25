/// Provides access to relational application data.
pub trait AppDbStore: Clone + Send + Sync + 'static {
    /// Returns the relational application-data pool.
    fn pool(&self) -> &sqlx::SqlitePool;
}
