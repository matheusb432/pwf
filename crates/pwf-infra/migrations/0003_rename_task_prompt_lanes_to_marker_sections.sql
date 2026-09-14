ALTER TABLE task_prompt_lanes RENAME TO task_marker_sections;
ALTER TABLE task_marker_sections RENAME COLUMN lane TO section;
