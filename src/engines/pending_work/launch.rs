// Launch-spec construction for the launch/new actions.

use super::model::Item;

fn is_adhoc(item: &Item) -> bool {
    item.id.starts_with("adhoc:")
}

fn report_closeout_command(id: &str) -> String {
    format!("pwf pw check --id {id} --report \"<brief result>\"")
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

pub(super) fn new_launch_spec(
    item: &Item,
    model: Option<&str>,
    thinking: Option<&str>,
) -> serde_json::Value {
    let success_label = format!(
        "[pending-work {}] {} :: {}",
        item.id, item.project, item.session
    );
    let launch_prompt = new_launch_prompt(item);
    let mut map = serde_json::Map::new();
    map.insert("id".into(), serde_json::json!(item.id));
    map.insert("project".into(), serde_json::json!(item.project));
    map.insert("session".into(), serde_json::json!(item.session));
    map.insert("name".into(), serde_json::json!(item.session));
    map.insert("title".into(), serde_json::json!(item.session));
    map.insert("successLabel".into(), serde_json::json!(success_label));
    map.insert("prompt".into(), serde_json::json!(item.prompt));
    map.insert("launchPrompt".into(), serde_json::json!(launch_prompt));
    let target = serde_json::json!({
        "type": "project",
        "projectPath": item.repo,
        "environment": { "type": "local" }
    });
    map.insert("target".into(), target);
    // note and line only when present
    map.insert("note".into(), serde_json::json!(item.note));
    map.insert("line".into(), serde_json::json!(item.line));
    if let Some(m) = model {
        map.insert("model".into(), serde_json::json!(m));
    }
    if let Some(t) = thinking {
        map.insert("thinking".into(), serde_json::json!(t));
    }
    serde_json::Value::Object(map)
}

pub(super) fn write_launch_spec(
    item: &Item,
    launch: &serde_json::Value,
    json: bool,
    model: Option<&str>,
    thinking: Option<&str>,
) -> String {
    if json {
        return serde_json::to_string_pretty(launch).unwrap();
    }
    let success_label = launch["successLabel"].as_str().unwrap_or("");
    let mut out = format!("READY TO LAUNCH {success_label}\n");
    if let Some(repo) = &item.repo {
        out.push_str(&format!("  repo: {repo}\n"));
    }
    out.push_str(&format!("  title: {}\n", item.session));
    if let Some(m) = model {
        out.push_str(&format!("  model: {m}\n"));
    } else {
        out.push_str("  model: repo/user default\n");
    }
    if let Some(t) = thinking {
        out.push_str(&format!("  thinking: {t}\n"));
    } else {
        out.push_str("  thinking: repo/user default\n");
    }
    let prompt_inline = item.prompt.replace('\n', " / ");
    out.push_str(&format!("  prompt: {prompt_inline}\n"));
    if !is_adhoc(item) {
        out.push_str("After the new thread is visible/running, mark this note checked with:\n");
        out.push_str(&format!("  {}\n", report_closeout_command(&item.id)));
        out.push_str("If a handoff or plan is the source of truth, close that out instead.\n");
    }
    out
}
