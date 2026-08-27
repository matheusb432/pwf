use std::hint::black_box;

use allocation_counter::{AllocationInfo, measure};

#[path = "../benches/obsidian_support/allocation_report.rs"]
mod allocation_report;
#[path = "../benches/obsidian_support/fixture.rs"]
mod fixture;
#[path = "../benches/obsidian_support/store_fixture.rs"]
mod store_fixture;
#[path = "../benches/obsidian_support/store_workload.rs"]
mod store_workload;

use allocation_report::AllocationCase;
use fixture::{DocumentSize, require};
use store_workload::{InsertWorkload, ReadWorkload, UpdateWorkload};

fn main() -> anyhow::Result<()> {
    store_workload::validate();
    allocation_report::run("obsidian-store-io", cases())
}

fn cases() -> Vec<AllocationCase> {
    let mut cases = Vec::new();
    for size in [DocumentSize::Small, DocumentSize::Large] {
        cases.push(AllocationCase::new(
            format!("read-task/{}", size.name()),
            move || measure_read(size),
        ));
    }
    cases.push(AllocationCase::new("list-tasks/64-small", measure_list));
    for size in [DocumentSize::Small, DocumentSize::Large] {
        cases.push(AllocationCase::new(
            format!("update-task/{}", size.name()),
            move || measure_update(size),
        ));
        cases.push(AllocationCase::new(
            format!("insert-task/{}", size.name()),
            move || measure_insert(size),
        ));
    }
    cases
}

fn measure_read(size: DocumentSize) -> AllocationInfo {
    let workload = ReadWorkload::single(size);
    measure(|| {
        drop(black_box(require(
            workload.get(),
            "reading allocation fixture",
        )));
    })
}

fn measure_list() -> AllocationInfo {
    let workload = ReadWorkload::many();
    measure(|| {
        drop(black_box(require(
            workload.list(),
            "listing allocation fixture",
        )));
    })
}

fn measure_update(size: DocumentSize) -> AllocationInfo {
    let mut workload = UpdateWorkload::new(size);
    measure(|| {
        require(workload.update(), "updating allocation fixture");
    })
}

fn measure_insert(size: DocumentSize) -> AllocationInfo {
    let mut workload = InsertWorkload::new(size);
    measure(|| {
        drop(black_box(require(
            workload.insert(),
            "inserting allocation fixture",
        )));
    })
}
