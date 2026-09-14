CREATE TABLE project_sources (
    id          INTEGER PRIMARY KEY,
    kind        TEXT NOT NULL CHECK (kind IN ('directory')),
    value       TEXT NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE (kind, value)
) STRICT;

CREATE TABLE projects (
    id                 TEXT PRIMARY KEY CHECK (
        length(id) BETWEEN 2 AND 4
        AND id NOT GLOB '*[^A-Z]*'
    ),
    project_source_id  INTEGER REFERENCES project_sources(id),
    title              TEXT NOT NULL COLLATE NOCASE UNIQUE,
    tasks_kind         TEXT NOT NULL CHECK (tasks_kind IN ('directory')),
    tasks_path         TEXT NOT NULL,
    created_at         TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    paused_at          TEXT,
    obsidian_vault     TEXT CHECK (obsidian_vault IS NULL OR length(trim(obsidian_vault)) > 0), snapshot_enabled INTEGER NOT NULL DEFAULT 0 CHECK (snapshot_enabled IN (0, 1)), last_task_number INTEGER
    CHECK (last_task_number BETWEEN 0 AND 9999),
    UNIQUE (tasks_kind, tasks_path)
) STRICT;

CREATE TABLE "task_marker_sections" (
    section    TEXT PRIMARY KEY CHECK (section IN (
        'goals',
        'context',
        'constraints',
        'done_when'
    )),
    marker  TEXT NOT NULL UNIQUE CHECK (
        length(marker) = 2
        AND substr(marker, 1, 1) = '/'
        AND substr(marker, 2, 1) GLOB '[A-Za-z]'
    ),
    header  TEXT NOT NULL UNIQUE CHECK (
        length(header) BETWEEN 1 AND 128
        AND header = trim(header)
        AND instr(header, char(10)) = 0
        AND instr(header, char(13)) = 0
    )
) STRICT;

CREATE VIEW active_projects AS
SELECT
    id,
    project_source_id,
    title,
    tasks_kind,
    tasks_path,
    created_at,
    obsidian_vault
FROM projects
WHERE paused_at IS NULL;
