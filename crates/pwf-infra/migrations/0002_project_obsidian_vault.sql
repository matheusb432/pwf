ALTER TABLE projects ADD COLUMN obsidian_vault TEXT CHECK (obsidian_vault IS NULL OR length(trim(obsidian_vault)) > 0);

DROP VIEW active_projects;
CREATE VIEW active_projects AS
SELECT id, project_source_id, title, tasks_kind, tasks_path, created_at, obsidian_vault
FROM projects
WHERE paused_at IS NULL;
