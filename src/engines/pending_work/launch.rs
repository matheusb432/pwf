// Launch prompt construction shared by session and verify.

use super::model::Item;

fn is_adhoc(item: &Item) -> bool {
    item.id.starts_with("adhoc:")
}

fn report_closeout_command(id: &str) -> String {
    format!("pwf check --id {id} --report \"<brief result>\"")
}

fn report_closeout_text(item: &Item) -> Option<String> {
    if is_adhoc(item) {
        return None;
    }
    Some(format!(
        "Closeout: if no handoff or plan is the source of truth for this item, check it done with a one-line report:\n{}",
        report_closeout_command(&item.id)
    ))
}

pub(super) fn new_launch_prompt(item: &Item) -> String {
    let mut out = format_prompt(item);
    if let Some(closeout) = report_closeout_text(item) {
        out.push_str("\n\n");
        out.push_str(&closeout);
    }
    out.trim().to_string()
}

fn format_prompt(item: &Item) -> String {
    format!(
        "Pending-work ID: {}\nProject: {}\n\n{}",
        item.id, item.project, item.prompt
    )
}
