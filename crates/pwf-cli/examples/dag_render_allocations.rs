use allocation_counter::{AllocationInfo, measure};

#[path = "../benches/dag_render_support/allocation_report.rs"]
mod allocation_report;
#[path = "../benches/dag_render_support/fixture.rs"]
mod fixture;

use allocation_report::AllocationCase;
use fixture::{DagRenderColor, FIXTURE_SCHEMA_VERSION, fixtures, validate_renderer};

const ALLOCATION_SAMPLE_COUNT: usize = 30;

fn main() -> anyhow::Result<()> {
    allocation_report::run(
        "dag-render-prepare",
        FIXTURE_SCHEMA_VERSION,
        ALLOCATION_SAMPLE_COUNT,
        cases(),
    )
}

fn cases() -> Vec<AllocationCase> {
    let mut cases = Vec::new();
    for fixture in fixtures() {
        validate_renderer(&fixture.task_dag, fixture.name);
        for color in DagRenderColor::ALL {
            let task_dag = fixture.task_dag.clone();
            cases.push(AllocationCase::new(
                format!("{}/{}", fixture.name, color.name()),
                move || measure_dag_render(&task_dag, color),
            ));
        }
    }
    cases
}

fn measure_dag_render(
    task_dag: &pwf_client::task::TaskDag,
    color: DagRenderColor,
) -> AllocationInfo {
    let task_dag = task_dag.clone();
    measure(|| pwf_cli::task::benchmark_dag_render_prepare(task_dag, color.color_on()))
}
