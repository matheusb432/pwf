use std::hint::black_box;

use allocation_counter::{AllocationInfo, measure};
use pwf_infra::obsidian::MarkdownFile;

#[path = "../benches/obsidian_support/allocation_report.rs"]
mod allocation_report;
#[path = "../benches/obsidian_support/fixture.rs"]
mod fixture;
#[path = "../benches/obsidian_support/markdown_file_workload.rs"]
mod markdown_file_workload;
#[path = "../benches/obsidian_support/markdown_fixture.rs"]
mod markdown_fixture;

use allocation_report::AllocationCase;
use fixture::{DocumentSize, require};
use markdown_file_workload::MarkdownReadWorkload;
use markdown_fixture::NodeMetadata;

fn main() -> anyhow::Result<()> {
    markdown_file_workload::validate();
    allocation_report::run("obsidian-frontmatter-read", cases())
}

fn cases() -> Vec<AllocationCase> {
    let mut cases = Vec::new();
    for size in [DocumentSize::Small, DocumentSize::Large] {
        let size_name = size.name();
        cases.push(AllocationCase::new(
            format!("typed/full-document/{size_name}"),
            move || measure_full_document(size),
        ));
        cases.push(AllocationCase::new(
            format!("typed/frontmatter-only/{size_name}"),
            move || measure_frontmatter_only(size),
        ));
        cases.push(AllocationCase::new(
            format!("open-path/borrowed/{size_name}"),
            move || measure_open_borrowed(size),
        ));
        cases.push(AllocationCase::new(
            format!("open-path/owned/{size_name}"),
            move || measure_open_owned(size),
        ));
    }
    cases
}

fn measure_full_document(size: DocumentSize) -> AllocationInfo {
    let workload = MarkdownReadWorkload::new(size);
    let path = workload.path.clone();
    measure(|| {
        let file = require(MarkdownFile::open(path), "opening complete Markdown file");
        drop(black_box(require(
            file.frontmatter::<NodeMetadata>(),
            "parsing complete Markdown file frontmatter",
        )));
    })
}

fn measure_frontmatter_only(size: DocumentSize) -> AllocationInfo {
    let workload = MarkdownReadWorkload::new(size);
    let path = workload.path.clone();
    measure(|| {
        drop(black_box(require(
            MarkdownFile::read_frontmatter::<NodeMetadata>(path),
            "reading bounded frontmatter",
        )));
    })
}

fn measure_open_borrowed(size: DocumentSize) -> AllocationInfo {
    let workload = MarkdownReadWorkload::new(size);
    measure(|| {
        drop(black_box(require(
            MarkdownFile::open(&workload.path),
            "opening Markdown file from a borrowed path",
        )));
    })
}

fn measure_open_owned(size: DocumentSize) -> AllocationInfo {
    let workload = MarkdownReadWorkload::new(size);
    let path = workload.path.clone();
    measure(|| {
        drop(black_box(require(
            MarkdownFile::open(path),
            "opening Markdown file from an owned path",
        )));
    })
}
