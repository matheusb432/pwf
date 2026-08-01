CREATE TABLE projects_next (
    id                 TEXT PRIMARY KEY CHECK (
        length(id) = 3
        AND id NOT GLOB '*[^A-Z]*'
    ),
    project_source_id  INTEGER NOT NULL REFERENCES project_sources(id),
    title              TEXT NOT NULL COLLATE NOCASE UNIQUE,
    tasks_kind         TEXT NOT NULL CHECK (tasks_kind IN ('directory')),
    tasks_path         TEXT NOT NULL,
    created_at         TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    paused_at          TEXT,
    UNIQUE (tasks_kind, tasks_path)
) STRICT;

INSERT INTO projects_next (
    id,
    project_source_id,
    title,
    tasks_kind,
    tasks_path,
    created_at,
    paused_at
)
SELECT
    id,
    project_source_id,
    title,
    tasks_kind,
    tasks_path,
    created_at,
    paused_at
FROM projects;

DROP VIEW active_projects;
DROP TABLE projects;
ALTER TABLE projects_next RENAME TO projects;

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
