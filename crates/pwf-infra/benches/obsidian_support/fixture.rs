use std::{fmt::Write as _, fs, path::PathBuf, sync::OnceLock};

use serde::{Deserialize, Serialize};
use tempfile::TempDir;

const FIXTURE_MANIFEST_SOURCE: &str = include_str!("../fixtures/obsidian-file-io.toml");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocumentSize {
    Small,
    Large,
}

impl DocumentSize {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Small => "small",
            Self::Large => "large",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct FixtureManifest {
    pub schema_version: u32,
    pub small_body_line_count: usize,
    pub large_body_line_count: usize,
    pub list_task_count: usize,
    pub allocation_sample_count: usize,
}

impl FixtureManifest {
    pub fn body_line_count(&self, size: DocumentSize) -> usize {
        match size {
            DocumentSize::Small => self.small_body_line_count,
            DocumentSize::Large => self.large_body_line_count,
        }
    }
}

pub fn manifest() -> &'static FixtureManifest {
    static MANIFEST: OnceLock<FixtureManifest> = OnceLock::new();
    MANIFEST.get_or_init(|| {
        require(
            toml::from_str(FIXTURE_MANIFEST_SOURCE),
            "parse fixture manifest",
        )
    })
}

pub fn document_body(size: DocumentSize) -> String {
    let line_count = manifest().body_line_count(size);
    let mut body = String::with_capacity(line_count * 80);
    for line_index in 0..line_count {
        let _ = writeln!(
            body,
            "Benchmark body line {line_index:05} retains [[PWF-0001]] and deterministic Markdown."
        );
    }
    body
}

pub fn temporary_directory(prefix: &str) -> TempDir {
    let root = benchmark_artifact_root().join("runtime");
    require(
        fs::create_dir_all(&root),
        "create benchmark runtime directory",
    );
    require(
        tempfile::Builder::new().prefix(prefix).tempdir_in(root),
        "create repository-local benchmark directory",
    )
}

pub fn benchmark_artifact_root() -> PathBuf {
    require(
        fs::canonicalize(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")),
        "resolving benchmark workspace root",
    )
    .join(".artifacts/benchmarks/obsidian-file-io")
}

pub fn require<T, Error>(result: Result<T, Error>, context: &str) -> T
where
    Error: std::fmt::Display,
{
    match result {
        Ok(value) => value,
        Err(error) => {
            eprintln!("benchmark setup failed while {context}: {error}");
            std::process::exit(1);
        }
    }
}
