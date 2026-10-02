use pwf_client::pb::{TaskMutationSummary, TaskStatus};
use pwf_models::settings::UserSettings;

use super::render_task_summary;

#[derive(Clone, Copy)]
pub(in crate::task) enum TaskMutationAction {
    Added,
    Cloned,
    Edited,
    Done,
    Cancelled,
    Activated,
    Backlogged,
    AlreadyActive,
    AlreadyBacklogged,
    Removed,
}

impl TaskMutationAction {
    fn label(self) -> &'static str {
        match self {
            Self::Added => "Added",
            Self::Cloned => "Cloned",
            Self::Edited => "Edited",
            Self::Done => "Done",
            Self::Cancelled => "Cancelled",
            Self::Activated => "Activated",
            Self::Backlogged => "Backlogged",
            Self::AlreadyActive => "Already active",
            Self::AlreadyBacklogged => "Already backlogged",
            Self::Removed => "Removed",
        }
    }
}

pub(in crate::task) fn render_mutation(
    action: TaskMutationAction,
    task_id: &str,
    task: Option<&TaskMutationSummary>,
    settings: &UserSettings,
    color_on: bool,
) -> anyhow::Result<String> {
    let Some(task) = task else {
        return Ok(format!(
            "{} task: {task_id}{}[title unavailable]",
            action.label(),
            settings.task_title_separator()
        ));
    };
    let status = TaskStatus::try_from(task.status)
        .ok()
        .filter(|status| *status != TaskStatus::Unspecified)
        .ok_or_else(|| anyhow::anyhow!("pwf-server returned an invalid task status"))?;
    let summary = render_task_summary(&task.id, &task.title, status, false, settings, color_on);
    Ok(format!("{} task: {summary}", action.label()))
}

#[cfg(test)]
mod tests {
    use pwf_models::settings::{NoteStatusColors, ProjectStatusColors, RgbColor, TaskStatusColors};

    use super::*;
    use crate::test_style::color_rgb;

    #[test]
    fn absent_summary_uses_the_confirmed_task_identifier() {
        assert_eq!(
            render_mutation(
                TaskMutationAction::Removed,
                "FOO-0001",
                None,
                &UserSettings::default(),
                true
            )
            .unwrap(),
            "Removed task: FOO-0001 :: [title unavailable]"
        );
    }

    #[test]
    fn mutations_share_list_status_colors_and_have_no_trailing_newline() {
        let colors = TaskStatusColors::new(
            Some(RgbColor::new(1, 2, 3)),
            Some(RgbColor::new(4, 5, 6)),
            Some(RgbColor::new(7, 8, 9)),
            Some(RgbColor::new(10, 11, 12)),
        );
        let settings = UserSettings::new(
            colors,
            ProjectStatusColors::default(),
            NoteStatusColors::default(),
            pwf_models::task::PriorityTier::Medium,
            pwf_models::task::order::OrderSpec::default(),
        );
        for (action, status, label, style) in [
            (
                TaskMutationAction::Backlogged,
                TaskStatus::Backlog,
                "Backlogged",
                color_rgb(10, 11, 12),
            ),
            (
                TaskMutationAction::AlreadyBacklogged,
                TaskStatus::Backlog,
                "Already backlogged",
                color_rgb(10, 11, 12),
            ),
            (
                TaskMutationAction::Added,
                TaskStatus::Active,
                "Added",
                color_rgb(1, 2, 3),
            ),
            (
                TaskMutationAction::Edited,
                TaskStatus::Active,
                "Edited",
                color_rgb(1, 2, 3),
            ),
            (
                TaskMutationAction::Done,
                TaskStatus::Done,
                "Done",
                color_rgb(4, 5, 6),
            ),
            (
                TaskMutationAction::Cancelled,
                TaskStatus::Cancelled,
                "Cancelled",
                color_rgb(7, 8, 9),
            ),
            (
                TaskMutationAction::Activated,
                TaskStatus::Active,
                "Activated",
                color_rgb(1, 2, 3),
            ),
            (
                TaskMutationAction::Removed,
                TaskStatus::Done,
                "Removed",
                color_rgb(4, 5, 6),
            ),
            (
                TaskMutationAction::AlreadyActive,
                TaskStatus::Active,
                "Already active",
                color_rgb(1, 2, 3),
            ),
        ] {
            let task = TaskMutationSummary {
                id: "FOO-0001".into(),
                title: "sample task".into(),
                status: status as i32,
            };
            let plain = render_mutation(action, "FOO-0001", Some(&task), &settings, false).unwrap();
            assert_eq!(plain, format!("{label} task: FOO-0001 :: sample task"));
            let colored =
                render_mutation(action, "FOO-0001", Some(&task), &settings, true).unwrap();
            assert_eq!(
                colored,
                format!("{label} task: {style}FOO-0001{style:#} :: sample task")
            );
        }
    }
}
