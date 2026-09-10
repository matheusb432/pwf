use pwf_client::pb::{TaskMutationSummary, TaskStatus};
use pwf_models::settings::TaskStatusColors;

use super::render_task_summary;

#[derive(Clone, Copy)]
pub(in crate::task) enum TaskMutationAction {
    Added,
    Cloned,
    Edited,
    Done,
    Cancelled,
    Reopened,
    Removed,
    Skipped,
}

impl TaskMutationAction {
    fn past_tense(self) -> &'static str {
        match self {
            Self::Added => "Added",
            Self::Cloned => "Cloned",
            Self::Edited => "Edited",
            Self::Done => "Done",
            Self::Cancelled => "Cancelled",
            Self::Reopened => "Reopened",
            Self::Removed => "Removed",
            Self::Skipped => "Skipped",
        }
    }
}

pub(in crate::task) fn render_mutation(
    action: TaskMutationAction,
    task_id: &str,
    task: Option<&TaskMutationSummary>,
    task_status_colors: TaskStatusColors,
    color_on: bool,
) -> anyhow::Result<String> {
    let Some(task) = task else {
        return Ok(format!(
            "{} task: {task_id} :: [title unavailable]",
            action.past_tense()
        ));
    };
    let status = TaskStatus::try_from(task.status)
        .ok()
        .filter(|status| *status != TaskStatus::Unspecified)
        .ok_or_else(|| anyhow::anyhow!("pwf-server returned an invalid task status"))?;
    let summary = render_task_summary(
        &task.id,
        &task.title,
        status,
        false,
        task_status_colors,
        color_on,
    );
    Ok(format!("{} task: {summary}", action.past_tense()))
}

#[cfg(test)]
mod tests {
    use pwf_models::settings::RgbColor;

    use super::*;
    use crate::test_style::color_rgb;

    #[test]
    fn legacy_receipts_report_success_without_inventing_title_or_status() {
        assert_eq!(
            render_mutation(
                TaskMutationAction::Removed,
                "FOO-0001",
                None,
                TaskStatusColors::default(),
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
        );
        for (action, status, label, style) in [
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
                TaskMutationAction::Reopened,
                TaskStatus::Active,
                "Reopened",
                color_rgb(1, 2, 3),
            ),
            (
                TaskMutationAction::Removed,
                TaskStatus::Done,
                "Removed",
                color_rgb(4, 5, 6),
            ),
            (
                TaskMutationAction::Skipped,
                TaskStatus::Active,
                "Skipped",
                color_rgb(1, 2, 3),
            ),
        ] {
            let task = TaskMutationSummary {
                id: "FOO-0001".into(),
                title: "sample task".into(),
                status: status as i32,
            };
            let plain = render_mutation(action, "FOO-0001", Some(&task), colors, false).unwrap();
            assert_eq!(plain, format!("{label} task: FOO-0001 :: sample task"));
            let colored = render_mutation(
                TaskMutationAction::Edited,
                "FOO-0001",
                Some(&task),
                colors,
                true,
            )
            .unwrap();
            assert_eq!(
                colored,
                format!("Edited task: {style}FOO-0001{style:#} :: sample task")
            );
        }
    }
}
