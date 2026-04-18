use super::domain::read_models::{ListResult, OpenItem};
use super::prereq;
use crate::config::Config;

/// "More" footer (no trailing newline); empty when nothing is hidden. Mentions the
/// hidden count and the `-n 0` escape hatch. ASCII-only for Linux+Windows consoles.
fn more_footer(hidden: usize) -> String {
    if hidden == 0 {
        return String::new();
    }
    format!("... and {hidden} more; run 'pwf pw -n 0' to show all")
}

pub(super) fn render_list(
    result: &ListResult,
    cfg: &Config,
    only_project: Option<&str>,
    json: bool,
    long: bool,
) -> String {
    if json {
        return serde_json::to_string_pretty(result.items()).unwrap();
    }
    if result.items().is_empty() {
        let target = only_project
            .map(|p| format!("{p} in {}", cfg.notes_dir))
            .unwrap_or_else(|| cfg.notes_dir.clone());
        return format!("No open pending-work prompts found in {target}.\n");
    }
    let mut out = String::new();
    let last_idx = result.items().len() - 1;
    for (idx, item) in result.items().iter().enumerate() {
        render_list_item(&mut out, item, cfg, long, idx == last_idx);
    }
    if result.hidden() > 0 {
        // ? Short mode leaves the last item without a trailing newline (preserved when
        // ? nothing is hidden); add one only here so the footer sits on its own line.
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(&more_footer(result.hidden()));
    }
    out
}

fn render_list_item(out: &mut String, item: &OpenItem, cfg: &Config, long: bool, last: bool) {
    let formatted = if last {
        format!("[{}] {} :: {}", item.id, item.project, item.session)
    } else {
        format!("[{}] {} :: {}\n", item.id, item.project, item.session)
    };
    out.push_str(&formatted);
    if !long {
        return;
    }
    let status = if item.launchable {
        "READY"
    } else {
        "NEEDS ATTENTION"
    };
    out.push_str(&format!("  status: {status}\n"));
    if item.needs_prompt {
        out.push_str("  status: NEEDS PROMPT\n");
    }
    match &item.repo {
        Some(r) if !r.is_empty() => out.push_str(&format!("  repo: {r}\n")),
        _ => out.push_str("  repo: (not configured)\n"),
    }
    out.push_str(&format!("  note: {}:{}\n", item.note, item.line));
    let prompt = item.prompt.replace("\r\n", " / ").replace('\n', " / ");
    out.push_str(&format!("  prompt: {prompt}\n"));
    if let Some(pq) = &item.prereq {
        let statuses = prereq::resolve(cfg, pq);
        if !statuses.is_empty() {
            out.push_str(&format!("  prereq: {}\n", prereq::list_summary(&statuses)));
        }
    }
    for issue in &item.issues {
        out.push_str(&format!("  issue: {issue}\n"));
    }
    if !item.launchable {
        out.push_str(&format!(
            "  fix: edit {} or config/pending-work.json\n",
            item.note
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn more_footer_empty_when_nothing_hidden() {
        assert_eq!(more_footer(0), "");
    }

    #[test]
    fn more_footer_mentions_count_and_escape_hatch() {
        let footer = more_footer(2);
        assert!(footer.contains("2 more"), "got: {footer}");
        assert!(footer.contains("-n 0"), "got: {footer}");
    }
}
