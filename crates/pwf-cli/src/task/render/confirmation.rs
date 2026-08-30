//! Formats mutation confirmations from stable operation identifiers.

use anstyle::AnsiColor;

use crate::render::paint;

pub(in crate::task) fn render_added(task_id: &str, color_on: bool) -> String {
    confirmation("Added pwf task", AnsiColor::Green, task_id, color_on)
}

pub(in crate::task) fn render_removed(task_id: &str, color_on: bool) -> String {
    confirmation("Removed pwf task", AnsiColor::Red, task_id, color_on)
}

pub(in crate::task) fn render_edited(task_id: &str, color_on: bool) -> String {
    confirmation("Edited pwf task", AnsiColor::Blue, task_id, color_on)
}

pub(super) fn render_review_task(task_id: &str) -> String {
    format!("ADDED PWF TASK [{task_id}]\n")
}

fn confirmation(label: &str, color: AnsiColor, task_id: &str, color_on: bool) -> String {
    format!("{label}: {}\n", paint(task_id, color, color_on))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirmations_render_only_the_stable_task_identifier() {
        assert_eq!(
            render_added("FOO-0001", false),
            "Added pwf task: **FOO-0001**\n"
        );
        assert_eq!(
            render_removed("FOO-0002", false),
            "Removed pwf task: **FOO-0002**\n"
        );
        assert_eq!(
            render_edited("FOO-0003", false),
            "Edited pwf task: **FOO-0003**\n"
        );
        assert_eq!(
            render_review_task("FOO-0004"),
            "ADDED PWF TASK [FOO-0004]\n"
        );
    }
}
