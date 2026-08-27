use std::{fs, path::PathBuf};

use pwf_infra::obsidian::{MarkdownFile, MarkdownFileError};
use tempfile::TempDir;

use super::{
    fixture::{DocumentSize, document_body, require, temporary_directory},
    markdown_fixture::{NodeMetadata, node_metadata, node_source},
};

pub struct MarkdownReadWorkload {
    _directory: TempDir,
    pub path: PathBuf,
    pub source: String,
    pub metadata: NodeMetadata,
}

impl MarkdownReadWorkload {
    pub fn new(size: DocumentSize) -> Self {
        let directory = temporary_directory("markdown-read-");
        let node_directory = directory.path().join("workspace/nodes");
        let path = node_directory.join("harbor.md");
        require(
            fs::create_dir_all(node_directory),
            "creating benchmark node directory",
        );
        let source = node_source(size);
        require(fs::write(&path, &source), "writing benchmark node");
        Self {
            _directory: directory,
            path,
            source,
            metadata: node_metadata(),
        }
    }

    pub fn open(&self) -> Result<MarkdownFile, MarkdownFileError> {
        MarkdownFile::open(&self.path)
    }

    pub fn opened(&self) -> MarkdownFile {
        require(self.open(), "opening benchmark Markdown file")
    }
}

pub struct MutationWorkload {
    _read: MarkdownReadWorkload,
    pub file: MarkdownFile,
}

impl MutationWorkload {
    pub fn new(size: DocumentSize) -> Self {
        let read = MarkdownReadWorkload::new(size);
        let file = read.opened();
        Self { _read: read, file }
    }

    pub fn set_status(&mut self) -> Result<(), MarkdownFileError> {
        self.file.set_property("status", "done")
    }
}

pub struct SaveWorkload {
    _read: MarkdownReadWorkload,
    pub file: MarkdownFile,
}

impl SaveWorkload {
    pub fn new(size: DocumentSize) -> Self {
        let read = MarkdownReadWorkload::new(size);
        let mut file = read.opened();
        require(
            file.set_property("status", "done"),
            "preparing benchmark Markdown mutation",
        );
        Self { _read: read, file }
    }

    pub fn save(&self) -> Result<(), MarkdownFileError> {
        self.file.save()
    }
}

pub struct CreateWorkload {
    _directory: TempDir,
    pub path: PathBuf,
    pub metadata: NodeMetadata,
    pub body: String,
}

impl CreateWorkload {
    pub fn new(size: DocumentSize) -> Self {
        let directory = temporary_directory("markdown-create-");
        let path = directory.path().join("workspace/nodes/harbor.md");
        Self {
            _directory: directory,
            path,
            metadata: node_metadata(),
            body: format!("# Harbor signal index\n\n{}", document_body(size)),
        }
    }

    pub fn create(&self) -> Result<MarkdownFile, MarkdownFileError> {
        MarkdownFile::create_new(&self.path, &self.metadata, &self.body)
    }
}

pub fn validate() {
    for size in [DocumentSize::Small, DocumentSize::Large] {
        let read = MarkdownReadWorkload::new(size);
        let file = read.opened();
        assert_eq!(file.source(), read.source);
        assert_eq!(
            require(
                file.frontmatter::<NodeMetadata>(),
                "parsing benchmark frontmatter",
            ),
            Some(read.metadata)
        );
        assert!(file.body().ends_with(&document_body(size)));

        let mut mutation = MutationWorkload::new(size);
        require(mutation.set_status(), "mutating benchmark frontmatter");
        assert!(mutation.file.source().contains("status: \"done\"\n"));
        assert!(mutation.file.source().ends_with(&document_body(size)));

        let save = SaveWorkload::new(size);
        require(save.save(), "saving benchmark Markdown file");
        assert_eq!(
            require(
                fs::read_to_string(save.file.path()),
                "reading saved benchmark Markdown file",
            ),
            save.file.source()
        );

        let create = CreateWorkload::new(size);
        let file = require(create.create(), "creating benchmark Markdown file");
        assert_eq!(
            require(
                file.frontmatter::<NodeMetadata>(),
                "parsing created benchmark frontmatter",
            ),
            Some(create.metadata)
        );
        assert!(file.body().ends_with(&document_body(size)));
    }
}
