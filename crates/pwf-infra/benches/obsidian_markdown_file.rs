use std::{hint::black_box, time::Duration};

use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use pwf_infra::obsidian::MarkdownFile;

#[path = "obsidian_support/fixture.rs"]
mod fixture;
#[path = "obsidian_support/markdown_file_workload.rs"]
mod markdown_file_workload;
#[path = "obsidian_support/markdown_fixture.rs"]
mod markdown_fixture;

use fixture::{DocumentSize, manifest};
use markdown_file_workload::{
    CreateWorkload, MarkdownReadWorkload, MutationWorkload, SaveWorkload,
};
use markdown_fixture::NodeMetadata;

const SAMPLE_SIZE: usize = 20;

fn obsidian_markdown_file(criterion: &mut Criterion) {
    markdown_file_workload::validate();
    eprintln!(
        "obsidian-markdown-file fixture_schema={}",
        manifest().schema_version
    );

    for size in [DocumentSize::Small, DocumentSize::Large] {
        benchmark_size(criterion, size);
    }
}

fn benchmark_size(criterion: &mut Criterion, size: DocumentSize) {
    let read = MarkdownReadWorkload::new(size);
    let source_byte_count = read.source.len() as u64;

    let mut open_group = criterion.benchmark_group("obsidian-markdown-file/open");
    open_group.throughput(Throughput::Bytes(source_byte_count));
    open_group.bench_with_input(
        BenchmarkId::from_parameter(size.name()),
        &size,
        |bencher, _| {
            bencher.iter_batched(
                || (),
                |()| black_box(MarkdownFile::open(&read.path)),
                BatchSize::SmallInput,
            );
        },
    );
    open_group.finish();

    let file = read.opened();
    let mut frontmatter_group = criterion.benchmark_group("obsidian-markdown-file/frontmatter");
    frontmatter_group.throughput(Throughput::Bytes(source_byte_count));
    frontmatter_group.bench_with_input(
        BenchmarkId::from_parameter(size.name()),
        &size,
        |bencher, _| {
            bencher.iter_batched(
                || (),
                |()| black_box(file.frontmatter::<NodeMetadata>()),
                BatchSize::SmallInput,
            );
        },
    );
    frontmatter_group.finish();

    let mut body_group = criterion.benchmark_group("obsidian-markdown-file/body");
    body_group.bench_with_input(
        BenchmarkId::from_parameter(size.name()),
        &size,
        |bencher, _| {
            bencher.iter(|| black_box(file.body()));
        },
    );
    body_group.finish();

    let mut mutation_group = criterion.benchmark_group("obsidian-markdown-file/set-property");
    mutation_group.bench_with_input(
        BenchmarkId::from_parameter(size.name()),
        &size,
        |bencher, size| {
            bencher.iter_batched_ref(
                || MutationWorkload::new(*size),
                |workload| black_box(workload.set_status()),
                BatchSize::LargeInput,
            );
        },
    );
    mutation_group.finish();

    let mut save_group = criterion.benchmark_group("obsidian-markdown-file/save");
    save_group.throughput(Throughput::Bytes(source_byte_count));
    save_group.bench_with_input(
        BenchmarkId::from_parameter(size.name()),
        &size,
        |bencher, size| {
            bencher.iter_batched_ref(
                || SaveWorkload::new(*size),
                |workload| black_box(workload.save()),
                BatchSize::LargeInput,
            );
        },
    );
    save_group.finish();

    let mut create_group = criterion.benchmark_group("obsidian-markdown-file/create-new");
    create_group.throughput(Throughput::Bytes(source_byte_count));
    create_group.bench_with_input(
        BenchmarkId::from_parameter(size.name()),
        &size,
        |bencher, size| {
            bencher.iter_batched_ref(
                || CreateWorkload::new(*size),
                |workload| black_box(workload.create()),
                BatchSize::LargeInput,
            );
        },
    );
    create_group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(SAMPLE_SIZE)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(2));
    targets = obsidian_markdown_file
}
criterion_main!(benches);
