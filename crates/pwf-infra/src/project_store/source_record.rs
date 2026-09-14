use pwf_models::project::ProjectSource;

pub(super) async fn get_or_insert(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    source: &ProjectSource,
) -> Result<i64, sqlx::Error> {
    let source_kind = source.kind().to_string();
    let source_value = source.value().as_ref();
    let source_id = sqlx::query_scalar!(
        r#"
        SELECT id AS "id!"
        FROM project_sources
        WHERE kind = ? AND value = ?
        "#,
        source_kind,
        source_value,
    )
    .fetch_optional(&mut **transaction)
    .await?;
    match source_id {
        Some(source_id) => Ok(source_id),
        None => sqlx::query!(
            r#"
            INSERT INTO project_sources (kind, value)
            VALUES (?, ?)
            "#,
            source_kind,
            source_value,
        )
        .execute(&mut **transaction)
        .await
        .map(|result| result.last_insert_rowid()),
    }
}
