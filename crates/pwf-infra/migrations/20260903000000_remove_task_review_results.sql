CREATE TABLE task_mutation_requests_next (
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

INSERT INTO task_mutation_requests_next (
    request_id,
    operation,
    fingerprint,
    task_id,
    state,
    outcome,
    created_at,
    completed_at
)
SELECT
    request_id,
    operation,
    fingerprint,
    task_id,
    state,
    outcome,
    created_at,
    completed_at
FROM task_mutation_requests;

DROP TABLE task_mutation_requests;
ALTER TABLE task_mutation_requests_next RENAME TO task_mutation_requests;
