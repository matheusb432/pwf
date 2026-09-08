ALTER TABLE task_mutation_requests ADD COLUMN task_title TEXT;
ALTER TABLE task_mutation_requests ADD COLUMN task_status TEXT
    CHECK (
        (task_title IS NULL AND task_status IS NULL)
        OR (task_title IS NOT NULL AND task_status IS NOT NULL
            AND task_status IN ('active', 'done', 'cancelled'))
    );
