use std::{hint::black_box, time::Duration};

use criterion::{
    BatchSize, Bencher, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main,
};

#[path = "dag_render_support/fixture.rs"]
mod fixture;

use fixture::{
    DagRenderColor, DagRenderFixture, FIXTURE_SCHEMA_VERSION, fixtures, require, validate_renderer,
};

const SAMPLE_SIZE: usize = 10;

fn dag_render(criterion: &mut Criterion) {
    eprintln!("dag-render fixture_schema={FIXTURE_SCHEMA_VERSION}");

    let mut group = criterion.benchmark_group("dag-render");
    for fixture in fixtures() {
        validate_renderer(&fixture.task_dag, fixture.name);
        group.throughput(Throughput::Elements(require(
            u64::try_from(fixture.task_dag.nodes().len() + fixture.task_dag.edges().len()),
            "converting the fixture element count to u64",
        )));
        for color in DagRenderColor::ALL {
            group.bench_with_input(
                BenchmarkId::new(fixture.name, color.name()),
                &(&fixture, color),
                benchmark_fixture,
            );
        }
    }
    group.finish();
}

fn benchmark_fixture(
    bencher: &mut Bencher<'_>,
    (fixture, color): &(&DagRenderFixture, DagRenderColor),
) {
    bencher.iter_batched(
        || fixture.task_dag.clone(),
        |task_dag| {
            black_box(pwf_cli::task::benchmark_dag_render(
                black_box(task_dag),
                color.color_on(),
            ))
        },
        BatchSize::SmallInput,
    );
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(SAMPLE_SIZE)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(3));
    targets = dag_render
}
criterion_main!(benches);
