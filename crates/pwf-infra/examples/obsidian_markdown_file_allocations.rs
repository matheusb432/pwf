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
use markdown_file_workload::{
    CreateWorkload, MarkdownReadWorkload, MutationWorkload, SaveWorkload,
};
use markdown_fixture::NodeMetadata;

fn main() -> anyhow::Result<()> {
    markdown_file_workload::validate();
    allocation_report::run("obsidian-markdown-file", cases())
}

fn cases() -> Vec<AllocationCase> {
    let mut cases = Vec::new();
    for size in [DocumentSize::Small, DocumentSize::Large] {
        let size_name = size.name();
        cases.push(AllocationCase::new(
            format!("open/{size_name}"),
            move || measure_open(size),
        ));
        cases.push(AllocationCase::new(
            format!("frontmatter/{size_name}"),
            move || measure_frontmatter(size),
        ));
        cases.push(AllocationCase::new(
            format!("body/{size_name}"),
            move || measure_body(size),
        ));
        cases.push(AllocationCase::new(
            format!("set-property/{size_name}"),
            move || measure_set_property(size),
        ));
        cases.push(AllocationCase::new(
            format!("save/{size_name}"),
            move || measure_save(size),
        ));
        cases.push(AllocationCase::new(
            format!("create-new/{size_name}"),
            move || measure_create(size),
        ));
    }
    cases
}

fn measure_open(size: DocumentSize) -> AllocationInfo {
    let workload = MarkdownReadWorkload::new(size);
    measure(|| {
        drop(black_box(require(
            MarkdownFile::open(&workload.path),
            "opening allocation fixture",
        )));
    })
}

fn measure_frontmatter(size: DocumentSize) -> AllocationInfo {
    let workload = MarkdownReadWorkload::new(size);
    let file = workload.opened();
    measure(|| {
        drop(black_box(require(
            file.frontmatter::<NodeMetadata>(),
            "parsing allocation fixture frontmatter",
        )));
    })
}

fn measure_body(size: DocumentSize) -> AllocationInfo {
    let workload = MarkdownReadWorkload::new(size);
    let file = workload.opened();
    measure(|| {
        black_box(file.body());
    })
}

fn measure_set_property(size: DocumentSize) -> AllocationInfo {
    let mut workload = MutationWorkload::new(size);
    measure(|| {
        require(
            workload.set_status(),
            "mutating allocation fixture frontmatter",
        );
    })
}

fn measure_save(size: DocumentSize) -> AllocationInfo {
    let workload = SaveWorkload::new(size);
    measure(|| {
        require(workload.save(), "saving allocation fixture");
    })
}

fn measure_create(size: DocumentSize) -> AllocationInfo {
    let workload = CreateWorkload::new(size);
    measure(|| {
        drop(black_box(require(
            workload.create(),
            "creating allocation fixture",
        )));
    })
}
