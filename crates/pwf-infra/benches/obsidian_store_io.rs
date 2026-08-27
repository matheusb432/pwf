use std::{hint::black_box, time::Duration};

use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

#[path = "obsidian_support/fixture.rs"]
mod fixture;
#[path = "obsidian_support/store_fixture.rs"]
mod store_fixture;
#[path = "obsidian_support/store_workload.rs"]
mod store_workload;

use fixture::{DocumentSize, manifest};
use store_workload::{InsertWorkload, ReadWorkload, UpdateWorkload};

const SAMPLE_SIZE: usize = 20;

fn obsidian_store_io(criterion: &mut Criterion) {
    store_workload::validate();
    eprintln!(
        "obsidian-store-io fixture_schema={}",
        manifest().schema_version
    );

    let mut read_group = criterion.benchmark_group("obsidian-store-io/read-task");
    for size in [DocumentSize::Small, DocumentSize::Large] {
        let workload = ReadWorkload::single(size);
        read_group.throughput(Throughput::Bytes(workload.expected_body.len() as u64));
        read_group.bench_with_input(
            BenchmarkId::from_parameter(size.name()),
            &size,
            |bencher, _| {
                bencher.iter_batched(|| (), |()| black_box(workload.get()), BatchSize::SmallInput);
            },
        );
    }
    read_group.finish();

    let workload = ReadWorkload::many();
    let mut list_group = criterion.benchmark_group("obsidian-store-io/list-tasks");
    list_group.throughput(Throughput::Elements(manifest().list_task_count as u64));
    list_group.bench_function("64-small", |bencher| {
        bencher.iter_batched(
            || (),
            |()| black_box(workload.list()),
            BatchSize::LargeInput,
        );
    });
    list_group.finish();

    let mut update_group = criterion.benchmark_group("obsidian-store-io/update-task");
    for size in [DocumentSize::Small, DocumentSize::Large] {
        update_group.throughput(Throughput::Bytes(fixture::document_body(size).len() as u64));
        update_group.bench_with_input(
            BenchmarkId::from_parameter(size.name()),
            &size,
            |bencher, size| {
                bencher.iter_batched_ref(
                    || UpdateWorkload::new(*size),
                    |workload| black_box(workload.update()),
                    BatchSize::LargeInput,
                );
            },
        );
    }
    update_group.finish();

    let mut insert_group = criterion.benchmark_group("obsidian-store-io/insert-task");
    for size in [DocumentSize::Small, DocumentSize::Large] {
        insert_group.throughput(Throughput::Bytes(fixture::document_body(size).len() as u64));
        insert_group.bench_with_input(
            BenchmarkId::from_parameter(size.name()),
            &size,
            |bencher, size| {
                bencher.iter_batched_ref(
                    || InsertWorkload::new(*size),
                    |workload| black_box(workload.insert()),
                    BatchSize::LargeInput,
                );
            },
        );
    }
    insert_group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(SAMPLE_SIZE)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(2));
    targets = obsidian_store_io
}
criterion_main!(benches);
