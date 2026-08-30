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
