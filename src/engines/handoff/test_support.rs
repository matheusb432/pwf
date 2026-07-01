//! Shared fixture helper for the handoff module tree's tests. The whole module
//! is gated `#[cfg(test)]` at its declaration in `mod.rs`.

pub(super) fn tempdir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "pwf_handoff_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
