use pwf_domain::project::{
    ProjectName, ProjectPrefix, ProjectSource, ProjectSourceKind, ProjectSourceValue, ProjectTasks,
    ProjectTasksKind, ProjectTasksPath,
};

/// Describes one persisted managed project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    /// Canonical project prefix.
    pub id: ProjectPrefix,
    /// Project title.
    pub title: ProjectName,
    /// Project source location.
    pub source: ProjectSource,
    /// Pending-work task location.
    pub tasks: ProjectTasks,
    /// RFC 3339 UTC creation timestamp.
    pub created_at: String,
    /// Reports whether the project is paused.
    pub is_paused: bool,
}

/// Describes the current project and whether a requested state transition changed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectStateChange {
    /// Current persisted project.
    pub project: Project,
    /// Reports whether the requested transition changed persisted state.
    pub changed: bool,
}

#[derive(Debug)]
pub(super) struct ProjectRow {
    pub(super) id: String,
    pub(super) title: String,
    pub(super) source_kind: String,
    pub(super) source_value: String,
    pub(super) tasks_kind: String,
    pub(super) tasks_path: String,
    pub(super) created_at: String,
    pub(super) is_paused: bool,
}

#[derive(Debug, thiserror::Error)]
#[error("persisted project {field} value {value:?} is invalid: {reason}")]
pub(super) struct ProjectRowError {
    field: &'static str,
    value: String,
    reason: String,
}

impl TryFrom<ProjectRow> for Project {
    type Error = ProjectRowError;

    fn try_from(row: ProjectRow) -> Result<Self, Self::Error> {
        let id = convert("id", row.id, ProjectPrefix::try_new)?;
        let title = convert("title", row.title, ProjectName::try_new)?;
        let source_kind = convert("source kind", row.source_kind, |value| {
            ProjectSourceKind::try_from(value.as_str())
        })?;
        let source_value = convert(
            "source value",
            row.source_value,
            ProjectSourceValue::try_new,
        )?;
        let tasks_kind = convert("tasks kind", row.tasks_kind, |value| {
            ProjectTasksKind::try_from(value.as_str())
        })?;
        let tasks_path = convert("tasks path", row.tasks_path, ProjectTasksPath::try_new)?;
        if row.created_at.is_empty() {
            return Err(ProjectRowError {
                field: "created_at",
                value: row.created_at,
                reason: "value cannot be empty".to_string(),
            });
        }

        Ok(Self {
            id,
            title,
            source: ProjectSource::new(source_kind, source_value),
            tasks: ProjectTasks::new(tasks_kind, tasks_path),
            created_at: row.created_at,
            is_paused: row.is_paused,
        })
    }
}

fn convert<T, E>(
    field: &'static str,
    value: String,
    conversion: impl FnOnce(String) -> Result<T, E>,
) -> Result<T, ProjectRowError>
where
    E: std::fmt::Display,
{
    conversion(value.clone()).map_err(|error| ProjectRowError {
        field,
        value,
        reason: error.to_string(),
    })
}
