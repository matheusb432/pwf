CREATE TABLE project_sources (
    id          INTEGER PRIMARY KEY,
    kind        TEXT NOT NULL CHECK (kind IN ('directory')),
    value       TEXT NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE (kind, value)
) STRICT;

CREATE TABLE projects (
    id                 TEXT PRIMARY KEY,
    project_source_id  INTEGER NOT NULL REFERENCES project_sources(id),
    title              TEXT NOT NULL UNIQUE,
    tasks_kind         TEXT NOT NULL CHECK (tasks_kind IN ('directory')),
    tasks_path         TEXT NOT NULL,
    created_at         TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    paused_at          TEXT,
    UNIQUE (tasks_kind, tasks_path)
) STRICT;

CREATE VIEW active_projects AS
SELECT
    id,
    project_source_id,
    title,
    tasks_kind,
    tasks_path,
    created_at
FROM projects
WHERE paused_at IS NULL;
