use super::domain::read_models::{ListResult, OpenItem};
use super::prereq;
use crate::config::Config;

/// "More" footer (no trailing newline); empty when nothing is hidden. Mentions the
/// hidden count and the `-n 0` escape hatch. ASCII-only for Linux+Windows consoles.
fn more_footer(hidden: usize) -> String {
    if hidden == 0 {
        return String::new();
    }
    format!("... and {hidden} more; run 'pwf -n 0' to show all")
}

pub(super) fn render_list(
    result: &ListResult,
    cfg: &Config,
    only_project: Option<&str>,
    long: bool,
    grouped: bool,
) -> String {
    if result.items().is_empty() {
        let target = only_project
            .map(|p| format!("{p} in {}", cfg.notes_dir))
            .unwrap_or_else(|| cfg.notes_dir.clone());
        return format!("No open pending-work prompts found in {target}.\n");
    }
    let mut out = String::new();
    if grouped {
        render_grouped_list(&mut out, result.items(), cfg, long);
    } else {
        let last_idx = result.items().len() - 1;
        for (idx, item) in result.items().iter().enumerate() {
            render_list_item(&mut out, item, cfg, long, idx == last_idx);
        }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RenderGroup {
    Default,
    LowPrio,
    Human,
    Future,
    Other,
}

impl RenderGroup {
    fn from_section(section: Option<&str>) -> Self {
        match section {
            None => Self::Default,
            Some("Low-prio") => Self::LowPrio,
            Some("Human") => Self::Human,
            Some("Future") => Self::Future,
            Some(_) => Self::Other,
        }
    }

    fn title(self) -> Option<&'static str> {
        match self {
            Self::Default => None,
            Self::LowPrio => Some("Low-prio"),
            Self::Human => Some("Human"),
            Self::Future => Some("Future"),
            Self::Other => Some("Other"),
        }
    }
}

const RENDER_GROUPS: [RenderGroup; 5] = [
    RenderGroup::Default,
    RenderGroup::LowPrio,
    RenderGroup::Human,
    RenderGroup::Future,
    RenderGroup::Other,
];

fn render_grouped_list(out: &mut String, items: &[OpenItem], cfg: &Config, long: bool) {
    let mut rendered_any = false;
    for group in RENDER_GROUPS {
        let group_items: Vec<&OpenItem> = items
            .iter()
            .filter(|item| RenderGroup::from_section(item.section.as_deref()) == group)
            .collect();
        if group_items.is_empty() {
            continue;
        }
        if rendered_any {
            out.push_str("\n\n");
        }
        if let Some(title) = group.title() {
            out.push_str(title);
            out.push('\n');
        }
        for (idx, item) in group_items.iter().enumerate() {
            render_list_item(out, item, cfg, long, idx + 1 == group_items.len());
        }
        rendered_any = true;
    }
}

fn render_list_item(out: &mut String, item: &OpenItem, cfg: &Config, long: bool, last: bool) {
    let formatted = if last && !long {
        format!("{} :: {}", item.id, item.session)
    } else {
        format!("{} :: {}\n", item.id, item.session)
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

    fn sample_item() -> OpenItem {
        OpenItem {
            id: "PWF-0064".to_string(),
            project: "pwf".to_string(),
            session: "make list commands formatting less redundant".to_string(),
            prompt: String::new(),
            repo: None,
            note: "pwf.md".to_string(),
            item_file: None,
            line: 1,
            format: String::new(),
            launchable: true,
            needs_prompt: false,
            issues: Vec::new(),
            section: None,
            prereq: None,
        }
    }

    fn empty_cfg() -> Config {
        Config {
            notes_dir: String::new(),
            projects: Default::default(),
            prefixes: Default::default(),
            work_prefix: String::new(),
            notes_dir_overrides: Default::default(),
        }
    }

    #[test]
    fn short_line_is_id_then_session_without_brackets_or_project() {
        let cfg = empty_cfg();
        let mut out = String::new();
        render_list_item(&mut out, &sample_item(), &cfg, false, true);
        assert_eq!(
            out,
            "PWF-0064 :: make list commands formatting less redundant"
        );
    }

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
