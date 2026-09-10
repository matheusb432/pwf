use pwf_marker_sections::{
    Adapter as _, LaneConfiguration, LaneConfigurationError, LaneDefinition, LaneDefinitionError,
    MarkdownAdapter, ParsedPrompt,
};
use pwf_wire::task::TaskLane;

const TASK_LANE_COUNT: usize = 4;
const TASK_LANE_NAMES: [&str; TASK_LANE_COUNT] = ["goals", "context", "constraints", "done_when"];

/// Reports an unreadable or invalid persisted task-prompt lane configuration.
#[derive(Debug, thiserror::Error)]
pub enum TaskPromptLanesError {
    #[error("cannot read task prompt lane configuration: {0}")]
    Database(#[from] sqlx::Error),
    #[error(
        "task prompt lane configuration must contain {expected:?} in that order; found {actual:?}"
    )]
    InvalidLaneSet {
        expected: [&'static str; TASK_LANE_COUNT],
        actual: Vec<String>,
    },
    #[error("invalid task prompt lane {lane:?}: {source}")]
    InvalidLane {
        lane: String,
        #[source]
        source: LaneDefinitionError,
    },
    #[error("invalid task prompt lane configuration: {0}")]
    InvalidConfiguration(#[from] LaneConfigurationError),
    #[error("task prompt lane configuration produced {actual} definitions; expected {expected}")]
    InvalidDefinitionCount { expected: usize, actual: usize },
}

#[derive(Debug)]
pub(super) struct TaskPromptLanes {
    configuration: LaneConfiguration<TASK_LANE_COUNT>,
}

impl TaskPromptLanes {
    pub(super) async fn load(pool: &sqlx::SqlitePool) -> Result<Self, TaskPromptLanesError> {
        let rows = sqlx::query!(
            r#"
            SELECT
                lane AS "lane!",
                marker AS "marker!",
                header AS "header!"
            FROM task_prompt_lanes
            ORDER BY CASE lane
                WHEN 'goals' THEN 0
                WHEN 'context' THEN 1
                WHEN 'constraints' THEN 2
                WHEN 'done_when' THEN 3
                ELSE 4
            END
            "#,
        )
        .fetch_all(pool)
        .await?;
        let actual = rows.iter().map(|row| row.lane.clone()).collect::<Vec<_>>();
        if actual.as_slice() != TASK_LANE_NAMES {
            return Err(TaskPromptLanesError::InvalidLaneSet {
                expected: TASK_LANE_NAMES,
                actual,
            });
        }
        let mut definitions = Vec::with_capacity(TASK_LANE_COUNT);
        for row in rows {
            definitions.push(lane_definition(row.lane, row.marker, row.header)?);
        }
        let definition_count = definitions.len();
        let lanes =
            definitions
                .try_into()
                .map_err(|_| TaskPromptLanesError::InvalidDefinitionCount {
                    expected: TASK_LANE_COUNT,
                    actual: definition_count,
                })?;
        Ok(Self {
            configuration: LaneConfiguration::try_new(lanes)?,
        })
    }

    pub(super) fn parse(&self, prompt: &str) -> ParsedPrompt<TASK_LANE_COUNT> {
        pwf_marker_sections::parse(prompt, &self.configuration)
    }

    pub(super) fn render(&self, parsed: &ParsedPrompt<TASK_LANE_COUNT>) -> String {
        MarkdownAdapter::new(&self.configuration).render(parsed)
    }

    pub(super) fn header(&self, lane: TaskLane) -> &str {
        self.configuration.lanes()[lane_index(lane)].header()
    }

    #[cfg(test)]
    pub(super) fn default_fixture() -> Self {
        Self {
            configuration: LaneConfiguration::try_new([
                LaneDefinition::try_new("/g", "Goals").unwrap(),
                LaneDefinition::try_new("/c", "Context").unwrap(),
                LaneDefinition::try_new("/n", "Constraints").unwrap(),
                LaneDefinition::try_new("/d", "Done When").unwrap(),
            ])
            .unwrap(),
        }
    }
}

fn lane_definition(
    lane: String,
    marker: String,
    header: String,
) -> Result<LaneDefinition, TaskPromptLanesError> {
    LaneDefinition::try_new(marker, header)
        .map_err(|source| TaskPromptLanesError::InvalidLane { lane, source })
}

const fn lane_index(lane: TaskLane) -> usize {
    match lane {
        TaskLane::Goal => 0,
        TaskLane::Context => 1,
        TaskLane::Constraint => 2,
        TaskLane::DoneWhen => 3,
    }
}
