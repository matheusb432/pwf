ALTER TABLE projects ADD COLUMN last_task_number INTEGER
    CHECK (last_task_number BETWEEN 0 AND 9999);
