//! Shared fixture helper for the handoff module tree's tests. The whole module
//! is gated `#[cfg(test)]` at its declaration in `mod.rs`.

/// A unique scoped temp dir; the returned guard deletes it on drop, so callers
/// must hold the binding for the fixture's lifetime.
pub(super) fn tempdir() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}
