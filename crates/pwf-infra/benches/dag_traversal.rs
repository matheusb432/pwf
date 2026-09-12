use std::{hint::black_box, time::Duration};

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use pwf_wire::task::TaskDagMode;

#[path = "dag_traversal_support/fixture.rs"]
mod fixture;

use fixture::{Shape, Workload};

fn dag_traversal(criterion: &mut Criterion) {
    eprintln!("dag-traversal fixture_schema=1 filesystem_cache=warm watched_cache=primed");
    for watched in [false, true] {
        benchmark_adapter(criterion, watched);
    }
}

fn benchmark_adapter(criterion: &mut Criterion, watched: bool) {
    let adapter = if watched { "watched" } else { "fresh" };
    let mut group = criterion.benchmark_group(format!("dag-traversal/{adapter}"));
    for shape in Shape::ALL {
        let workload = Workload::new(shape, watched);
        workload.validate();
        let allocations = allocation_counter::measure(|| {
            black_box(workload.dependencies());
        });
        eprintln!(
            "allocations {adapter}/{}/gather {allocations:?}",
            shape.name()
        );
        group.bench_function(BenchmarkId::new(shape.name(), "gather"), |bencher| {
            bencher.iter(|| black_box(workload.dependencies()));
        });
        for (name, mode) in [
            ("blocked-by", TaskDagMode::BlockedBy),
            ("blocks", TaskDagMode::Blocks),
            ("full", TaskDagMode::Full),
        ] {
            let allocations = allocation_counter::measure(|| {
                black_box(workload.graph(mode));
            });
            eprintln!(
                "allocations {adapter}/{}/{name} {allocations:?}",
                shape.name()
            );
            group.bench_function(BenchmarkId::new(shape.name(), name), |bencher| {
                bencher.iter(|| black_box(workload.graph(mode)));
            });
        }
    }
    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(10)
        .warm_up_time(Duration::from_millis(500))
        .measurement_time(Duration::from_secs(1));
    targets = dag_traversal
}
criterion_main!(benches);
