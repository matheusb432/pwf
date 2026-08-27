use std::{hint::black_box, path::PathBuf, time::Duration};

use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use pwf_infra::obsidian::{MarkdownFile, MarkdownFileError};

#[path = "obsidian_support/fixture.rs"]
mod fixture;
#[path = "obsidian_support/markdown_file_workload.rs"]
mod markdown_file_workload;
#[path = "obsidian_support/markdown_fixture.rs"]
mod markdown_fixture;

use fixture::{DocumentSize, manifest};
use markdown_file_workload::MarkdownReadWorkload;
use markdown_fixture::NodeMetadata;

const SAMPLE_SIZE: usize = 20;

fn obsidian_frontmatter_read(criterion: &mut Criterion) {
    markdown_file_workload::validate();
    eprintln!(
        "obsidian-frontmatter-read fixture_schema={}",
        manifest().schema_version
    );

    for size in [DocumentSize::Small, DocumentSize::Large] {
        benchmark_size(criterion, size);
    }
}

fn benchmark_size(criterion: &mut Criterion, size: DocumentSize) {
    let workload = MarkdownReadWorkload::new(size);

    let mut typed_group = criterion.benchmark_group("obsidian-frontmatter-read/typed");
    typed_group.bench_with_input(
        BenchmarkId::new("full-document", size.name()),
        &size,
        |bencher, _| {
            bencher.iter_batched(
                || workload.path.clone(),
                |path| black_box(read_full_document(path)),
                BatchSize::SmallInput,
            );
        },
    );
    typed_group.bench_with_input(
        BenchmarkId::new("frontmatter-only", size.name()),
        &size,
        |bencher, _| {
            bencher.iter_batched(
                || workload.path.clone(),
                |path| black_box(MarkdownFile::read_frontmatter::<NodeMetadata>(path)),
                BatchSize::SmallInput,
            );
        },
    );
    typed_group.finish();

    let mut path_group = criterion.benchmark_group("obsidian-frontmatter-read/open-path");
    path_group.bench_with_input(
        BenchmarkId::new("borrowed", size.name()),
        &size,
        |bencher, _| {
            bencher.iter_batched(
                || (),
                |()| black_box(MarkdownFile::open(&workload.path)),
                BatchSize::SmallInput,
            );
        },
    );
    path_group.bench_with_input(
        BenchmarkId::new("owned", size.name()),
        &size,
        |bencher, _| {
            bencher.iter_batched(
                || workload.path.clone(),
                |path| black_box(MarkdownFile::open(path)),
                BatchSize::SmallInput,
            );
        },
    );
    path_group.finish();
}

fn read_full_document(path: PathBuf) -> Result<Option<NodeMetadata>, MarkdownFileError> {
    MarkdownFile::open(path)?.frontmatter()
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(SAMPLE_SIZE)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(2));
    targets = obsidian_frontmatter_read
}
criterion_main!(benches);
