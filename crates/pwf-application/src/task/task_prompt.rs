#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskLane {
    Goal,
    Context,
    Constraint,
    DoneWhen,
}

impl TaskLane {
    fn label(self) -> &'static str {
        match self {
            Self::Goal => "goal",
            Self::Context => "context",
            Self::Constraint => "constraint",
            Self::DoneWhen => "done when",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TaskLaneValueError {
    #[error("{} cannot be empty.", lane.label())]
    Empty { lane: TaskLane },
    #[error("{} must be a single line.", lane.label())]
    Multiline { lane: TaskLane },
}

impl TaskLaneValueError {
    #[must_use]
    pub fn lane(&self) -> TaskLane {
        match self {
            Self::Empty { lane } | Self::Multiline { lane } => *lane,
        }
    }

    #[must_use]
    pub fn reason(&self) -> &'static str {
        match self {
            Self::Empty { .. } => "cannot be empty.",
            Self::Multiline { .. } => "must be a single line.",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskLanes {
    pub(super) goals: Vec<String>,
    pub(super) context: Vec<String>,
    pub(super) constraints: Vec<String>,
    pub(super) done_when: Vec<String>,
}

impl TaskLanes {
    /// Constructs ordered task lanes after trimming each single-line value.
    ///
    /// # Errors
    ///
    /// Returns [`TaskLaneValueError`] when any value is blank or contains a line break.
    pub fn try_new(
        goals: Vec<String>,
        context: Vec<String>,
        constraints: Vec<String>,
        done_when: Vec<String>,
    ) -> Result<Self, TaskLaneValueError> {
        Ok(Self {
            goals: normalize_lane(TaskLane::Goal, goals)?,
            context: normalize_lane(TaskLane::Context, context)?,
            constraints: normalize_lane(TaskLane::Constraint, constraints)?,
            done_when: normalize_lane(TaskLane::DoneWhen, done_when)?,
        })
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.goals.is_empty()
            && self.context.is_empty()
            && self.constraints.is_empty()
            && self.done_when.is_empty()
    }
}

fn normalize_lane(lane: TaskLane, values: Vec<String>) -> Result<Vec<String>, TaskLaneValueError> {
    values
        .into_iter()
        .map(|value| {
            if value.contains(['\n', '\r']) {
                return Err(TaskLaneValueError::Multiline { lane });
            }
            let value = value.trim().to_string();
            if value.is_empty() {
                return Err(TaskLaneValueError::Empty { lane });
            }
            Ok(value)
        })
        .collect()
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskLaneEdits {
    pub(super) additions: TaskLanes,
    pub(super) removals: Vec<TaskLane>,
}

impl TaskLaneEdits {
    #[must_use]
    pub fn new(additions: TaskLanes, removals: impl IntoIterator<Item = TaskLane>) -> Self {
        let mut normalized_removals = Vec::new();
        for lane in removals {
            if !normalized_removals.contains(&lane) {
                normalized_removals.push(lane);
            }
        }
        Self {
            additions,
            removals: normalized_removals,
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.additions.is_empty() && self.removals.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::{TaskLaneValueError, TaskLanes};

    #[test]
    fn lanes_trim_outer_whitespace_and_preserve_literal_markers() {
        let lanes = TaskLanes::try_new(
            vec!["  keep /c literal  ".to_string()],
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
        .unwrap();

        assert_eq!(lanes.goals, vec!["keep /c literal"]);
    }

    #[test]
    fn lanes_reject_blank_and_multiline_values() {
        assert!(matches!(
            TaskLanes::try_new(vec!["  ".to_string()], Vec::new(), Vec::new(), Vec::new()),
            Err(TaskLaneValueError::Empty { .. })
        ));
        assert!(matches!(
            TaskLanes::try_new(
                Vec::new(),
                vec!["one\ntwo".to_string()],
                Vec::new(),
                Vec::new(),
            ),
            Err(TaskLaneValueError::Multiline { .. })
        ));
    }
}
