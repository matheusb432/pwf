//! Provides shared handoff test fixtures.

/// Creates a temporary directory deleted when its guard drops.
pub(super) fn tempdir() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}
