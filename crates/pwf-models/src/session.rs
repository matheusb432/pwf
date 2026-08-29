use std::{collections::HashSet, fmt};

use nutype::nutype;
use thiserror::Error;

use crate::{project::ProjectId, task::TaskId};

/// Maximum Unicode scalar count accepted for one pushed prompt.
pub const PUSHED_PROMPT_CHAR_MAX: usize = 2000;

/// Maximum number of tasks accepted by one agent session.
pub const SESSION_TASK_ID_LIMIT: usize = 5;

/// Stores the task IDs dispatched together in one agent session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionTaskIds(Vec<TaskId>);

impl SessionTaskIds {
    /// Creates one bounded, unique, single-project task-ID collection.
    ///
    /// Input order is retained for prompt construction. Compound session
    /// identity is sorted independently through [`Self::identity`].
    ///
    /// # Errors
    ///
    /// Returns [`SessionTaskIdsError`] when the collection is empty, exceeds
    /// the session limit, repeats an ID, or spans more than one project.
    pub fn try_new(
        task_ids: impl IntoIterator<Item = TaskId>,
    ) -> Result<Self, SessionTaskIdsError> {
        let task_ids = task_ids.into_iter().collect::<Vec<_>>();
        let Some(first) = task_ids.first() else {
            return Err(SessionTaskIdsError::Empty);
        };
        if task_ids.len() > SESSION_TASK_ID_LIMIT {
            return Err(SessionTaskIdsError::TooMany {
                count: task_ids.len(),
                max: SESSION_TASK_ID_LIMIT,
            });
        }

        let expected_project = first.project_id().clone();
        let mut seen = HashSet::with_capacity(task_ids.len());
        if let Some(id) = task_ids
            .iter()
            .find(|task_id| !seen.insert((*task_id).clone()))
        {
            return Err(SessionTaskIdsError::Duplicate { id: id.clone() });
        }
        if let Some(id) = task_ids
            .iter()
            .find(|task_id| task_id.project_id() != &expected_project)
        {
            return Err(SessionTaskIdsError::MixedProjects {
                id: id.clone(),
                expected: expected_project,
                actual: id.project_id().clone(),
            });
        }

        Ok(Self(task_ids))
    }

    /// Returns task IDs in their supplied order.
    #[must_use]
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &TaskId> {
        self.0.iter()
    }

    /// Returns the first task ID.
    #[must_use]
    pub fn first(&self) -> &TaskId {
        &self.0[0]
    }

    /// Returns whether this collection contains one task.
    #[must_use]
    pub fn is_singleton(&self) -> bool {
        self.0.len() == 1
    }

    /// Returns the shared project ID.
    #[must_use]
    pub fn project_id(&self) -> &ProjectId {
        self.first().project_id()
    }

    /// Returns the stable session identity.
    ///
    /// Singleton identity remains the canonical task ID for compatibility.
    /// Compound identity uses ascending compact lowercase task IDs.
    #[must_use]
    pub fn identity(&self) -> String {
        if self.is_singleton() {
            return self.first().to_string();
        }

        let mut task_ids = self.0.iter().collect::<Vec<_>>();
        task_ids.sort_unstable();
        task_ids
            .into_iter()
            .map(|task_id| {
                format!(
                    "{}{}",
                    task_id.project_id().as_ref().to_ascii_lowercase(),
                    task_id.number()
                )
            })
            .collect::<Vec<_>>()
            .join(",")
    }
}

/// Reports an invalid collection of task IDs for one session.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SessionTaskIdsError {
    #[error("a session requires at least one task ID")]
    Empty,
    #[error("a session accepts at most {max} task IDs (received {count})")]
    TooMany { count: usize, max: usize },
    #[error("duplicate session task ID '{id}'")]
    Duplicate { id: TaskId },
    #[error(
        "session task '{id}' belongs to project '{actual}', but all tasks must belong to '{expected}'"
    )]
    MixedProjects {
        id: TaskId,
        expected: ProjectId,
        actual: ProjectId,
    },
}

/// Stores bounded, non-empty session prompt text.
#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = PUSHED_PROMPT_CHAR_MAX),
    derive(Debug, Clone, PartialEq, Eq, AsRef, Display, FromStr),
)]
pub struct PushedPrompt(String);

/// Stores the rendered title assigned to an agent session.
#[nutype(derive(Debug, Clone, PartialEq, Eq, AsRef, Display))]
pub struct SessionThreadTitle(String);

/// Stores the complete prompt passed to an agent launch.
#[nutype(derive(Debug, Clone, PartialEq, Eq, AsRef, Display))]
pub struct LaunchPrompt(String);

/// Identifies the runtime working directory for one agent session.
#[nutype(derive(Debug, Clone, PartialEq, Eq, AsRef, Display))]
pub struct SessionWorkingDirectory(String);

/// Selects a supported agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Agent {
    Claude,
    Codex,
}

/// Selects where an agent runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchMode {
    Inline,
    Multiplexer,
}

/// Selects optional launch-prompt directives.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LaunchDirectives {
    pub worktree: bool,
    pub autonomous: bool,
}

/// Selects the reasoning effort for one agent session.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SessionEffort {
    Low,
    Medium,
    #[default]
    High,
    XHigh,
    Max,
}

impl fmt::Display for SessionEffort {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::XHigh => "xhigh",
            Self::Max => "max",
        })
    }
}

/// Selects an optional agent model override.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AgentModel(Option<String>);

impl From<Option<String>> for AgentModel {
    fn from(value: Option<String>) -> Self {
        Self(
            value
                .map(|model| model.trim().to_string())
                .filter(|model| !model.is_empty() && model != Self::MODEL_DEFAULT),
        )
    }
}

impl AgentModel {
    pub const MODEL_DEFAULT: &str = "default";

    #[must_use]
    pub fn into_inner(self) -> Option<String> {
        self.0
    }

    #[must_use]
    pub fn as_deref(&self) -> Option<&str> {
        self.0.as_deref()
    }
}

impl fmt::Display for AgentModel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0.as_deref().unwrap_or(Self::MODEL_DEFAULT))
    }
}

#[cfg(test)]
mod tests {
    use super::{AgentModel, PUSHED_PROMPT_CHAR_MAX, PushedPrompt, SessionTaskIds};

    #[test]
    fn default_blank_and_absent_models_are_no_override() {
        assert_eq!(AgentModel::from(None::<String>).into_inner(), None);
        assert_eq!(
            AgentModel::from(Some("default".to_string())).into_inner(),
            None
        );
        assert_eq!(AgentModel::from(Some("  ".to_string())).into_inner(), None);
        assert_eq!(
            AgentModel::from(Some(" gpt-5.6 ".to_string())).into_inner(),
            Some("gpt-5.6".to_string())
        );
    }

    #[test]
    fn pushed_prompt_trims_outer_whitespace_and_preserves_internal_formatting() {
        let prompt = PushedPrompt::try_new("  first line\n  indented line  ").unwrap();

        assert_eq!(prompt.as_ref(), "first line\n  indented line");
    }

    #[test]
    fn pushed_prompt_rejects_blank_and_oversized_values() {
        assert!(PushedPrompt::try_new(" \n\t ").is_err());
        assert!(PushedPrompt::try_new("é".repeat(PUSHED_PROMPT_CHAR_MAX)).is_ok());
        assert!(PushedPrompt::try_new("é".repeat(PUSHED_PROMPT_CHAR_MAX + 1)).is_err());
    }

    #[test]
    fn session_task_ids_preserve_input_order_and_sort_compound_identity() {
        let ids =
            SessionTaskIds::try_new(["foo23".parse().unwrap(), "foo15".parse().unwrap()]).unwrap();

        assert_eq!(
            ids.iter().map(ToString::to_string).collect::<Vec<_>>(),
            ["FOO-0023", "FOO-0015"]
        );
        assert_eq!(ids.identity(), "foo15,foo23");
    }

    #[test]
    fn singleton_session_identity_preserves_the_existing_canonical_task_id() {
        let ids = SessionTaskIds::try_new(["foo15".parse().unwrap()]).unwrap();

        assert_eq!(ids.identity(), "FOO-0015");
    }

    #[test]
    fn session_task_ids_reject_invalid_collections() {
        assert!(SessionTaskIds::try_new(Vec::new()).is_err());
        assert!(
            SessionTaskIds::try_new(["foo1".parse().unwrap(), "foo1".parse().unwrap(),]).is_err()
        );
        assert!(
            SessionTaskIds::try_new(["foo1".parse().unwrap(), "bar2".parse().unwrap(),]).is_err()
        );
        assert!(
            SessionTaskIds::try_new([
                "foo1".parse().unwrap(),
                "foo2".parse().unwrap(),
                "foo3".parse().unwrap(),
                "foo4".parse().unwrap(),
                "foo5".parse().unwrap(),
                "foo6".parse().unwrap(),
            ])
            .is_err()
        );
    }

    #[test]
    fn session_task_ids_accept_the_five_task_limit() {
        assert!(
            SessionTaskIds::try_new([
                "foo1".parse().unwrap(),
                "foo2".parse().unwrap(),
                "foo3".parse().unwrap(),
                "foo4".parse().unwrap(),
                "foo5".parse().unwrap(),
            ])
            .is_ok()
        );
    }
}
