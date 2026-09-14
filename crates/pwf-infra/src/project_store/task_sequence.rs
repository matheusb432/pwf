use pwf_application::ports::project_store::TaskSequenceError;
use pwf_models::{project::ProjectId, task::TaskId};
use sqlx::SqliteConnection;

pub(super) async fn reserve(
    project: &ProjectId,
    connection: &mut SqliteConnection,
) -> Result<Option<TaskId>, TaskSequenceError> {
    let id = project.as_ref();
    // Consume SQLITE_DONE so commit errors cannot be lost when the returning row arrives.
    let number = sqlx::query_scalar!(
        r#"UPDATE projects SET last_task_number = last_task_number + 1
        WHERE id = ? AND last_task_number < 9999
        RETURNING last_task_number AS "number!: i64""#,
        id,
    )
    .fetch_all(&mut *connection)
    .await
    .map_err(|source| TaskSequenceError::Unexpected(source.into()))?
    .pop();
    if let Some(number) = number {
        return TaskId::try_new(format!("{project}-{number:04}"))
            .map(Some)
            .map_err(|source| TaskSequenceError::Unexpected(source.into()));
    }
    let row = sqlx::query!("SELECT last_task_number FROM projects WHERE id = ?", id)
        .fetch_optional(connection)
        .await
        .map_err(|source| TaskSequenceError::Unexpected(source.into()))?
        .ok_or_else(|| TaskSequenceError::ProjectNotFound {
            id: project.clone(),
        })?;
    if row.last_task_number == Some(9999) {
        return Err(TaskSequenceError::Exhausted {
            id: project.clone(),
        });
    }
    Ok(None)
}

pub(super) async fn advance(
    project: &ProjectId,
    highest: Option<&TaskId>,
    connection: &mut SqliteConnection,
) -> Result<(), TaskSequenceError> {
    let id = project.as_ref();
    let number = i64::from(highest.map_or(0, TaskId::number));
    let result = sqlx::query!(
        "UPDATE projects SET last_task_number = MAX(COALESCE(last_task_number, 0), ?) WHERE id = ?",
        number,
        id,
    )
    .execute(connection)
    .await
    .map_err(|source| TaskSequenceError::Unexpected(source.into()))?;
    if result.rows_affected() == 0 {
        return Err(TaskSequenceError::ProjectNotFound {
            id: project.clone(),
        });
    }
    Ok(())
}
