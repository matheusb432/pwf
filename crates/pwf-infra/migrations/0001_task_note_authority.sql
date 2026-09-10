DROP TABLE task_mutation_requests;

ALTER TABLE projects
ADD COLUMN snapshot_enabled INTEGER NOT NULL DEFAULT 0 CHECK (snapshot_enabled IN (0, 1));
