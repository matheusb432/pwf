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
    project_source_id  INTEGER NOT NULL REFERENCES project_sources(id),
    title              TEXT NOT NULL COLLATE NOCASE UNIQUE,
    tasks_kind         TEXT NOT NULL CHECK (tasks_kind IN ('directory')),
    tasks_path         TEXT NOT NULL,
    created_at         TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    paused_at          TEXT,
    UNIQUE (tasks_kind, tasks_path)
) STRICT;

CREATE TABLE task_mutation_requests (
    request_id   TEXT PRIMARY KEY CHECK (
        length(request_id) BETWEEN 1 AND 64
        AND request_id NOT GLOB '*[^0-9A-Za-z_.-]*'
    ),
    operation    TEXT NOT NULL CHECK (operation IN (
        'create',
        'update',
        'cancel',
        'complete',
        'delete',
        'reopen'
    )),
    fingerprint  TEXT NOT NULL CHECK (
        length(fingerprint) = 64
        AND fingerprint NOT GLOB '*[^0-9a-f]*'
    ),
    task_id      TEXT NOT NULL CHECK (length(task_id) BETWEEN 1 AND 64),
    state        TEXT NOT NULL DEFAULT 'pending' CHECK (state IN ('pending', 'completed')),
    outcome      TEXT,
    created_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    completed_at TEXT,
    CHECK (
        (
            state = 'pending'
            AND outcome IS NULL
            AND completed_at IS NULL
        )
        OR (
            state = 'completed'
            AND outcome IS NOT NULL
            AND completed_at IS NOT NULL
        )
    ),
    CHECK (
        outcome IS NULL
        OR (operation = 'create' AND outcome = 'created')
        OR (operation = 'update' AND outcome = 'updated')
        OR (operation = 'cancel' AND outcome = 'cancelled')
        OR (operation = 'complete' AND outcome = 'completed')
        OR (operation = 'delete' AND outcome IN ('deleted', 'aborted'))
        OR (operation = 'reopen' AND outcome IN ('reopened', 'already_active', 'aborted'))
    )
) STRICT;

CREATE TABLE task_prompt_lanes (
    lane    TEXT PRIMARY KEY CHECK (lane IN (
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

INSERT INTO task_prompt_lanes (lane, marker, header)
VALUES
    ('goals', '/g', 'Goals'),
    ('context', '/c', 'Context'),
    ('constraints', '/n', 'Constraints'),
    ('done_when', '/d', 'Done When');

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
