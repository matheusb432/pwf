use super::fixture::{DocumentSize, document_body};

pub fn task_source(task_number: usize, size: DocumentSize) -> String {
    let id = format!("PWF-{task_number:04}");
    format!(
        concat!(
            "---\n",
            "id: {id}\n",
            "status: active\n",
            "title: benchmark task {task_number:04}\n",
            "project: pwf\n",
            "created_at: 2026-08-27T12:34:56Z\n",
            "blocked_by: [\"[[AUX-0001]]\"]\n",
            "effort: medium\n",
            "tags: [\"benchmark\", \"obsidian\"]\n",
            "---\n\n",
            "{}"
        ),
        document_body(size),
        id = id,
        task_number = task_number,
    )
}
