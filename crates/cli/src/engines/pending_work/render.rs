use std::fmt::Write;

use anstyle::AnsiColor;
use pwf_domain::pending_work::{
    ListResult, PendingWorkItemView, WorkItemStatus, WorkItemStatusFilter,
};

use super::{
    color::{ID_ORANGE, paint},
    prereq,
};
use crate::config::Config;

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
    status_filter: WorkItemStatusFilter,
    long: bool,
    grouped: bool,
    on: bool,
) -> String {
    if result.items.is_empty() {
        let target = only_project.map_or_else(
            || cfg.notes_dir.clone(),
            |p| format!("{p} in {}", cfg.notes_dir),
        );
        return match status_filter {
            WorkItemStatusFilter::Exact(WorkItemStatus::Active) => {
                format!("No open pending-work prompts found in {target}.\n")
            }
            WorkItemStatusFilter::Exact(status) => {
                format!("No pending-work prompts with status {status} found in {target}.\n")
            }
            WorkItemStatusFilter::All => {
                format!("No pending-work prompts found in {target}.\n")
            }
        };
    }
    let mut out = String::new();
    if grouped {
        render_grouped_list(&mut out, &result.items, cfg, status_filter, long, on);
    } else {
        let last_idx = result.items.len() - 1;
        for (idx, item) in result.items.iter().enumerate() {
            render_list_item(
                &mut out,
                item,
                cfg,
                status_filter,
                long,
                idx == last_idx,
                on,
            );
        }
    }
    if result.hidden > 0 {
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(&more_footer(result.hidden));
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

fn render_grouped_list(
    out: &mut String,
    items: &[PendingWorkItemView],
    cfg: &Config,
    status_filter: WorkItemStatusFilter,
    long: bool,
    on: bool,
) {
    let mut rendered_any = false;
    for group in RENDER_GROUPS {
        let group_items: Vec<&PendingWorkItemView> = items
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
            render_list_item(
                out,
                item,
                cfg,
                status_filter,
                long,
                idx + 1 == group_items.len(),
                on,
            );
        }
        rendered_any = true;
    }
}

fn render_status(status: WorkItemStatus, on: bool) -> String {
    let text = status.to_string();
    if !on {
        return text;
    }
    let color: anstyle::Color = match status {
        WorkItemStatus::Active => ID_ORANGE.into(),
        WorkItemStatus::Done => AnsiColor::Green.into(),
        WorkItemStatus::Cancelled => AnsiColor::Red.into(),
    };
    paint(&text, color, true)
}

fn status_annotation(
    status: WorkItemStatus,
    status_filter: WorkItemStatusFilter,
    on: bool,
) -> String {
    if status_filter != WorkItemStatusFilter::All {
        return String::new();
    }
    format!(" ({})", render_status(status, on))
}

fn render_list_item(
    out: &mut String,
    item: &PendingWorkItemView,
    cfg: &Config,
    status_filter: WorkItemStatusFilter,
    long: bool,
    last: bool,
    on: bool,
) {
    // Plain output is a raw-text contract; Markdown emphasis is reserved for ANSI rendering.
    let id = if on {
        paint(&item.id, ID_ORANGE, true)
    } else {
        item.id.clone()
    };
    let annotation = status_annotation(item.status, status_filter, on);
    let formatted = if last && !long {
        format!("{id} :: {}{annotation}", item.session)
    } else {
        format!("{id} :: {}{annotation}\n", item.session)
    };
    out.push_str(&formatted);
    if !long {
        return;
    }
    let _ = writeln!(out, "  status: {}", render_status(item.status, on));
    if item.status == WorkItemStatus::Active {
        let launch = if item.launchable {
            "READY"
        } else {
            "NEEDS ATTENTION"
        };
        let _ = writeln!(out, "  launch: {launch}");
        if item.needs_prompt {
            out.push_str("  launch: NEEDS PROMPT\n");
        }
    }
    match &item.repo {
        Some(r) if !r.is_empty() => {
            let _ = writeln!(out, "  repo: {r}");
        }
        _ => out.push_str("  repo: (not configured)\n"),
    }
    let _ = writeln!(out, "  note: {}:{}", item.note, item.line);
    let prompt = item.prompt.replace("\r\n", " / ").replace('\n', " / ");
    let _ = writeln!(out, "  prompt: {prompt}");
    if let Some(pq) = &item.prereq {
        let statuses = prereq::resolve(cfg, pq);
        if !statuses.is_empty() {
            let _ = writeln!(out, "  prereq: {}", prereq::list_summary(&statuses));
        }
    }
    if let Some(e) = &item.effort {
        let _ = writeln!(out, "  effort: {e}");
    }
    if let Some(tags) = &item.tags {
        let _ = writeln!(out, "  tags: {tags}");
    }
    if item.status == WorkItemStatus::Active {
        for issue in &item.issues {
            let _ = writeln!(out, "  issue: {issue}");
        }
        if !item.launchable {
            let _ = writeln!(out, "  fix: edit {} or config/pending-work.json", item.note);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use pwf_domain::pending_work::{WorkItemStatus, WorkItemStatusFilter};

    use super::*;

    fn sample_item() -> PendingWorkItemView {
        PendingWorkItemView {
            id: "PWF-0064".to_string(),
            project: "pwf".to_string(),
            status: WorkItemStatus::Active,
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
            effort: None,
            tags: None,
            created: None,
        }
    }

    fn empty_cfg() -> Config {
        Config {
            notes_dir: String::new(),
            projects: BTreeMap::default(),
            prefixes: BTreeMap::default(),
            work_prefix: String::new(),
            notes_dir_overrides: BTreeMap::default(),
        }
    }

    fn render_item_for_filter(
        item: &PendingWorkItemView,
        status_filter: WorkItemStatusFilter,
        long: bool,
        on: bool,
    ) -> String {
        let cfg = empty_cfg();
        let mut output = String::new();
        render_list_item(&mut output, item, &cfg, status_filter, long, true, on);
        output
    }

    #[test]
    fn short_line_is_id_then_session_without_brackets_or_project() {
        let cfg = empty_cfg();
        let mut out = String::new();
        render_list_item(
            &mut out,
            &sample_item(),
            &cfg,
            WorkItemStatusFilter::default(),
            false,
            true,
            false,
        );
        assert_eq!(
            out,
            "PWF-0064 :: make list commands formatting less redundant"
        );
    }

    #[test]
    fn colored_id_is_orange_and_bold() {
        let cfg = empty_cfg();
        let mut out = String::new();
        render_list_item(
            &mut out,
            &sample_item(),
            &cfg,
            WorkItemStatusFilter::default(),
            false,
            true,
            true,
        );
        assert!(out.contains('\u{1b}'), "got: {out}");
        assert!(out.contains("PWF-0064"), "got: {out}");
    }

    #[test]
    fn all_status_short_lines_append_plain_lifecycle_annotations() {
        for (status, expected) in [
            (WorkItemStatus::Active, "(active)"),
            (WorkItemStatus::Done, "(done)"),
            (WorkItemStatus::Cancelled, "(cancelled)"),
        ] {
            let mut item = sample_item();
            item.status = status;
            let output = render_item_for_filter(&item, WorkItemStatusFilter::All, false, false);
            assert!(output.ends_with(expected), "{output}");
            assert!(!output.contains('\u{1b}'), "{output}");
        }
    }

    #[test]
    fn exact_status_short_lines_keep_the_existing_shape() {
        let output = render_item_for_filter(
            &sample_item(),
            WorkItemStatusFilter::Exact(WorkItemStatus::Active),
            false,
            false,
        );
        assert_eq!(
            output,
            "PWF-0064 :: make list commands formatting less redundant"
        );
    }

    #[test]
    fn all_status_annotations_use_distinct_lifecycle_colors() {
        let mut outputs = Vec::new();
        for status in [
            WorkItemStatus::Active,
            WorkItemStatus::Done,
            WorkItemStatus::Cancelled,
        ] {
            let mut item = sample_item();
            item.status = status;
            outputs.push(render_item_for_filter(
                &item,
                WorkItemStatusFilter::All,
                false,
                true,
            ));
        }

        assert!(outputs[0].contains("38;5;208"), "{}", outputs[0]);
        assert!(outputs[0].contains("active"), "{}", outputs[0]);
        assert!(outputs[1].contains("\u{1b}[32m"), "{}", outputs[1]);
        assert!(outputs[1].contains("done"), "{}", outputs[1]);
        assert!(outputs[2].contains("\u{1b}[31m"), "{}", outputs[2]);
        assert!(outputs[2].contains("cancelled"), "{}", outputs[2]);
    }

    #[test]
    fn long_form_separates_lifecycle_from_active_launch_readiness() {
        let output = render_item_for_filter(
            &sample_item(),
            WorkItemStatusFilter::Exact(WorkItemStatus::Active),
            true,
            false,
        );

        assert!(output.contains("  status: active\n"), "{output}");
        assert!(output.contains("  launch: READY\n"), "{output}");
    }

    #[test]
    fn closed_long_form_omits_active_launch_diagnostics() {
        let mut item = sample_item();
        item.status = WorkItemStatus::Done;
        item.launchable = false;
        item.needs_prompt = true;
        item.issues = vec!["missing repository".to_string()];

        let output = render_item_for_filter(
            &item,
            WorkItemStatusFilter::Exact(WorkItemStatus::Done),
            true,
            false,
        );

        assert!(output.contains("  status: done\n"), "{output}");
        assert!(!output.contains("launch:"), "{output}");
        assert!(!output.contains("issue:"), "{output}");
        assert!(!output.contains("fix:"), "{output}");
    }

    #[test]
    fn empty_result_text_reflects_the_selected_lifecycle_filter() {
        let result = ListResult {
            items: Vec::new(),
            hidden: 0,
        };
        let mut cfg = empty_cfg();
        cfg.notes_dir = "notes".to_string();

        for (filter, expected) in [
            (
                WorkItemStatusFilter::Exact(WorkItemStatus::Active),
                "No open pending-work prompts found in notes.\n",
            ),
            (
                WorkItemStatusFilter::Exact(WorkItemStatus::Done),
                "No pending-work prompts with status done found in notes.\n",
            ),
            (
                WorkItemStatusFilter::Exact(WorkItemStatus::Cancelled),
                "No pending-work prompts with status cancelled found in notes.\n",
            ),
            (
                WorkItemStatusFilter::All,
                "No pending-work prompts found in notes.\n",
            ),
        ] {
            assert_eq!(
                render_list(&result, &cfg, None, filter, false, false, false),
                expected
            );
        }
    }

    #[test]
    fn long_form_shows_effort_line_when_present() {
        let cfg = empty_cfg();
        let mut item = sample_item();
        item.effort = Some("3".to_string());
        let mut out = String::new();
        render_list_item(
            &mut out,
            &item,
            &cfg,
            WorkItemStatusFilter::default(),
            true,
            true,
            false,
        );
        assert!(out.contains("  effort: 3\n"), "got: {out}");
    }

    #[test]
    fn long_form_omits_effort_line_when_absent() {
        let cfg = empty_cfg();
        let mut out = String::new();
        render_list_item(
            &mut out,
            &sample_item(),
            &cfg,
            WorkItemStatusFilter::default(),
            true,
            true,
            false,
        );
        assert!(!out.contains("effort:"), "got: {out}");
    }

    #[test]
    fn long_form_shows_raw_tags_line_when_present() {
        let cfg = empty_cfg();
        let mut item = sample_item();
        item.tags = Some("[SQLite, hand-edited]".to_string());
        let mut out = String::new();
        render_list_item(
            &mut out,
            &item,
            &cfg,
            WorkItemStatusFilter::default(),
            true,
            true,
            false,
        );
        assert!(out.contains("  tags: [SQLite, hand-edited]\n"), "{out}");
    }

    #[test]
    fn long_form_omits_tags_line_when_absent() {
        let cfg = empty_cfg();
        let mut out = String::new();
        render_list_item(
            &mut out,
            &sample_item(),
            &cfg,
            WorkItemStatusFilter::default(),
            true,
            true,
            false,
        );
        assert!(!out.contains("tags:"), "{out}");
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
