use pwf_models::project::ProjectId;

/// Requests the task-body sections for a project, or the global selection when omitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GetTaskBodySections {
    pub project_id: Option<ProjectId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskBodyItemStyle {
    Bullet,
    Numbered,
    Paragraph,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskBodySection {
    pub marker: String,
    pub header: String,
    /// Markdown ATX heading level from 1 through 6.
    pub heading_level: u8,
    pub item_style: TaskBodyItemStyle,
}

/// Names the effective preset and lists its sections in rendering order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskBodySections {
    pub preset: String,
    pub sections: Vec<TaskBodySection>,
}
