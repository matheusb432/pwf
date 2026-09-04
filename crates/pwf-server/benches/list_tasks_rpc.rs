use std::{hint::black_box, time::Duration};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

#[path = "list_tasks_rpc_support/fixture.rs"]
mod fixture;
#[allow(dead_code)]
#[path = "../tests/support/server.rs"]
mod server;
#[path = "list_tasks_rpc_support/workload.rs"]
mod workload;

use fixture::FixtureManifest;
use workload::ListTasksRpcWorkload;

const RUNTIME_WORKER_THREAD_COUNT: usize = 2;
const SAMPLE_SIZE: usize = 20;

fn list_tasks_rpc(criterion: &mut Criterion) {
    let manifest = require(FixtureManifest::parse(), "parse task-list fixture manifest");
    eprintln!(
        "list-tasks-rpc fixture_schema={} runtime_worker_threads={RUNTIME_WORKER_THREAD_COUNT}",
        manifest.schema_version()
    );
    let runtime = require(build_runtime(), "build task-list benchmark runtime");
    let mut group = criterion.benchmark_group("list-tasks-rpc");

    for workload_spec in manifest.workloads() {
        let workload = require(
            runtime.block_on(ListTasksRpcWorkload::start(&manifest, workload_spec)),
            "start task-list RPC workload",
        );
        let validation = require(
            runtime.block_on(workload.measure()),
            "prime and validate task-list RPC workload",
        );
        require(
            workload.validate(&validation),
            "validate task-list RPC workload",
        );

        group.throughput(Throughput::Elements(
            u64::try_from(workload_spec.task_count_total())
                .unwrap_or_else(|error| benchmark_failure("convert task throughput", error)),
        ));
        group.bench_with_input(
            BenchmarkId::from_parameter(workload_spec.name()),
            &workload,
            |bencher, workload| {
                bencher.to_async(&runtime).iter(|| async {
                    black_box(require(
                        workload.measure().await,
                        "measure task-list RPC workload",
                    ))
                });
            },
        );

        require(
            runtime.block_on(workload.finish()),
            "stop task-list benchmark server",
        );
    }
    group.finish();
}

fn build_runtime() -> std::io::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(RUNTIME_WORKER_THREAD_COUNT)
        .enable_all()
        .build()
}

fn require<T, Error>(result: Result<T, Error>, context: &str) -> T
where
    Error: std::fmt::Display,
{
    result.unwrap_or_else(|error| benchmark_failure(context, error))
}

fn benchmark_failure(context: &str, error: impl std::fmt::Display) -> ! {
    eprintln!("benchmark failed while {context}: {error}");
    std::process::exit(1);
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(SAMPLE_SIZE)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(3));
    targets = list_tasks_rpc
}
criterion_main!(benches);
