DROP VIEW active_projects;

CREATE TABLE projects_nullable (
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
    obsidian_vault     TEXT CHECK (obsidian_vault IS NULL OR length(trim(obsidian_vault)) > 0),
    UNIQUE (tasks_kind, tasks_path)
) STRICT;

INSERT INTO projects_nullable (id, project_source_id, title, tasks_kind, tasks_path, created_at, paused_at, obsidian_vault)
SELECT id, project_source_id, title, tasks_kind, tasks_path, created_at, paused_at, obsidian_vault FROM projects;

DROP TABLE projects;
ALTER TABLE projects_nullable RENAME TO projects;

CREATE VIEW active_projects AS
SELECT id, project_source_id, title, tasks_kind, tasks_path, created_at, obsidian_vault
FROM projects WHERE paused_at IS NULL;
