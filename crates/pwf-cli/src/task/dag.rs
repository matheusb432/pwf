use clap::{Args, ValueEnum};
use pwf_client::{
    pb::{GetTaskDagRequest, TaskDagMode},
    task::TaskClient,
};
use pwf_models::settings::TaskStatusColors;

use super::{Identifier, StatusChoice};
use crate::console::Console;

mod output;

pub(super) fn render(task_dag: pwf_client::task::TaskDag, color_on: bool) -> String {
    output::render(task_dag, None, TaskStatusColors::default(), color_on)
}

pub(super) fn benchmark_prepare(task_dag: pwf_client::task::TaskDag, color_on: bool) {
    if color_on {
        drop(std::hint::black_box(output::prepare_colored(
            task_dag,
            None,
            TaskStatusColors::default(),
        )));
    } else {
        drop(std::hint::black_box(output::prepare(task_dag, None)));
    }
}

#[derive(Debug, Clone, Copy, Default, ValueEnum)]
enum ModeChoice {
    #[default]
    BlockedBy,
    Blocks,
    Full,
}

impl ModeChoice {
    const fn wire_value(self) -> TaskDagMode {
        match self {
            Self::BlockedBy => TaskDagMode::BlockedBy,
            Self::Blocks => TaskDagMode::Blocks,
            Self::Full => TaskDagMode::Full,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum NodeFieldChoice {
    Title,
    Status,
}

#[derive(Args, Debug)]
pub struct Arguments {
    #[command(flatten)]
    identifier: Identifier,
    /// Limit traversal to N dependency edges from the selected task.
    #[arg(long, value_name = "N", value_parser = clap::builder::RangedU64ValueParser::<u32>::new().range(1..))]
    depth: Option<u32>,
    /// Select blockers, dependents, or both graph directions.
    #[arg(long, value_enum, default_value = "blocked-by")]
    mode: ModeChoice,
    /// Filter surrounding nodes by one lifecycle status.
    #[arg(long, value_enum, default_value = "all")]
    status: StatusChoice,
    /// Include one additional field in each task node.
    #[arg(long = "with", value_enum, value_name = "WITH")]
    node_field: Option<NodeFieldChoice>,
}

pub(super) async fn run(
    arguments: &Arguments,
    console: Console,
    task_status_colors: TaskStatusColors,
    client: &TaskClient,
) -> anyhow::Result<String> {
    let id = arguments.identifier.id();
    let graph = client
        .get_task_dag(GetTaskDagRequest {
            id: id.to_string(),
            depth: arguments.depth,
            status: arguments.status.filter() as i32,
            mode: arguments.mode.wire_value() as i32,
        })
        .await
        .map_err(crate::rpc_error)?;
    Ok(output::render(
        graph,
        arguments.node_field,
        task_status_colors,
        console.color(),
    ))
}
