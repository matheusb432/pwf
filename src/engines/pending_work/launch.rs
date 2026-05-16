// Launch-spec construction for the launch/new actions.

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
    let mut out = format!(
        "Thread title: {}\nPending-work ID: {}\nProject: {}\n\n{}",
        item.session, item.id, item.project, item.prompt
    );
    if let Some(closeout) = report_closeout_text(item) {
        out.push_str("\n\n");
        out.push_str(&closeout);
    }
    out.trim().to_string()
}

pub(super) fn write_launch_spec(
    item: &Item,
    model: Option<&str>,
    thinking: Option<&str>,
) -> String {
    let success_label = format!(
        "[pending-work {}] {} :: {}",
        item.id, item.project, item.session
    );
    let mut out = format!("READY TO LAUNCH {success_label}\n");
    if let Some(repo) = &item.repo {
        out.push_str(&format!("  repo: {repo}\n"));
    }
    out.push_str(&format!("  title: {}\n", item.session));
    out.push_str(&format!(
        "  model: {}\n",
        model.unwrap_or("repo/user default")
    ));
    out.push_str(&format!(
        "  thinking: {}\n",
        thinking.unwrap_or("repo/user default")
    ));
    let prompt_inline = item.prompt.replace('\n', " / ");
    out.push_str(&format!("  prompt: {prompt_inline}\n"));
    if !is_adhoc(item) {
        out.push_str("After the new thread is visible/running, mark this note checked with:\n");
        out.push_str(&format!("  {}\n", report_closeout_command(&item.id)));
        out.push_str("If a handoff or plan is the source of truth, close that out instead.\n");
    }
    out
}
