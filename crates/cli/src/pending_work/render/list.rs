use std::fmt::Write;

use pwf_application::pending_work::{
    GetPendingWorkOk, PendingWorkItemView, PrerequisiteStatus, StatusFilter,
};
use pwf_domain::pending_work::WorkItemStatus;

use super::{StatusPlacement, render_item_summary, render_status};

pub(in crate::pending_work) fn render_list(
    result: &GetPendingWorkOk,
    location: &str,
    status_filter: StatusFilter,
    long: bool,
    grouped: bool,
    on: bool,
) -> String {
    if result.items.is_empty() {
        return match status_filter {
            StatusFilter::Exact(WorkItemStatus::Active) => {
                format!("No open pending-work prompts found in {location}.\n")
            }
            StatusFilter::Exact(status) => {
                format!("No pending-work prompts with status {status} found in {location}.\n")
            }
            StatusFilter::All => {
                format!("No pending-work prompts found in {location}.\n")
            }
        };
    }
    let mut out = String::new();
    if grouped {
        render_grouped_list(&mut out, &result.items, status_filter, long, on);
    } else {
        let last_idx = result.items.len() - 1;
        for (idx, item) in result.items.iter().enumerate() {
            render_list_item(&mut out, item, status_filter, long, idx == last_idx, on);
        }
    }
    if result.hidden > 0 {
        if !out.ends_with('\n') {
            out.push('\n');
        }
        let _ = write!(
            out,
            "... and {} more; use '--all' to list everything",
            result.hidden
        );
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
    status_filter: StatusFilter,
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
                status_filter,
                long,
                idx + 1 == group_items.len(),
                on,
            );
        }
        rendered_any = true;
    }
}

fn prerequisite_status_summary(statuses: &[PrerequisiteStatus]) -> String {
    statuses
        .iter()
        .map(|prerequisite| {
            let status = prerequisite
                .status
                .map_or_else(|| "missing".to_string(), |status| status.to_string());
            format!("{} ({status})", prerequisite.id)
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn render_list_item(
    out: &mut String,
    item: &PendingWorkItemView,
    status_filter: StatusFilter,
    long: bool,
    last: bool,
    on: bool,
) {
    let status = if status_filter == StatusFilter::All {
        Some((item.status, StatusPlacement::AfterIdentifier))
    } else {
        None
    };
    let summary = render_item_summary(&item.id, &item.session, status, on);
    let formatted = if last && !long {
        summary
    } else {
        format!("{summary}\n")
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
    if !item.prerequisite_statuses.is_empty() {
        let _ = writeln!(
            out,
            "  prereq: {}",
            prerequisite_status_summary(&item.prerequisite_statuses)
        );
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
            let _ = writeln!(
                out,
                "  fix: edit {} or update the managed project record",
                item.note
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use pwf_application::pending_work::{PrerequisiteStatus, StatusFilter};
    use pwf_domain::pending_work::{WorkItemId, WorkItemStatus};

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
            prerequisite_statuses: Vec::new(),
            effort: None,
            tags: None,
            created: None,
        }
    }

    fn render_item_for_filter(
        item: &PendingWorkItemView,
        status_filter: StatusFilter,
        long: bool,
        on: bool,
    ) -> String {
        let mut output = String::new();
        render_list_item(&mut output, item, status_filter, long, true, on);
        output
    }

    #[test]
    fn short_line_is_id_then_session_without_brackets_or_project() {
        let mut out = String::new();
        render_list_item(
            &mut out,
            &sample_item(),
            StatusFilter::default(),
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
        let mut out = String::new();
        render_list_item(
            &mut out,
            &sample_item(),
            StatusFilter::default(),
            false,
            true,
            true,
        );
        assert!(out.contains('\u{1b}'), "got: {out}");
        assert!(out.contains("PWF-0064"), "got: {out}");
    }

    #[test]
    fn all_status_short_lines_place_plain_lifecycle_after_the_identifier() {
        for (status, expected) in [
            (
                WorkItemStatus::Active,
                "PWF-0064 [active] :: make list commands formatting less redundant",
            ),
            (
                WorkItemStatus::Done,
                "PWF-0064 [done] :: make list commands formatting less redundant",
            ),
            (
                WorkItemStatus::Cancelled,
                "PWF-0064 [cancelled] :: make list commands formatting less redundant",
            ),
        ] {
            let mut item = sample_item();
            item.status = status;
            let output = render_item_for_filter(&item, StatusFilter::All, false, false);
            assert_eq!(output, expected);
            assert!(!output.contains('\u{1b}'), "{output}");
        }
    }

    #[test]
    fn exact_status_short_lines_keep_the_existing_shape() {
        let output = render_item_for_filter(
            &sample_item(),
            StatusFilter::Exact(WorkItemStatus::Active),
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
                StatusFilter::All,
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
            StatusFilter::Exact(WorkItemStatus::Active),
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
            StatusFilter::Exact(WorkItemStatus::Done),
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
        let result = GetPendingWorkOk {
            items: Vec::new(),
            hidden: 0,
            project: None,
            project_task_path: None,
            status_filter: StatusFilter::default(),
            grouped: false,
        };
        for (filter, expected) in [
            (
                StatusFilter::Exact(WorkItemStatus::Active),
                "No open pending-work prompts found in notes.\n",
            ),
            (
                StatusFilter::Exact(WorkItemStatus::Done),
                "No pending-work prompts with status done found in notes.\n",
            ),
            (
                StatusFilter::Exact(WorkItemStatus::Cancelled),
                "No pending-work prompts with status cancelled found in notes.\n",
            ),
            (
                StatusFilter::All,
                "No pending-work prompts found in notes.\n",
            ),
        ] {
            assert_eq!(
                render_list(&result, "notes", filter, false, false, false),
                expected
            );
        }
    }

    #[test]
    fn long_form_shows_effort_line_when_present() {
        let mut item = sample_item();
        item.effort = Some("high".to_string());
        let mut out = String::new();
        render_list_item(&mut out, &item, StatusFilter::default(), true, true, false);
        assert!(out.contains("  effort: high\n"), "got: {out}");
    }

    #[test]
    fn long_form_formats_typed_prerequisite_statuses() {
        let mut item = sample_item();
        item.prerequisite_statuses = vec![
            PrerequisiteStatus {
                id: WorkItemId::try_new("CFG-0014").unwrap(),
                status: Some(WorkItemStatus::Done),
            },
            PrerequisiteStatus {
                id: WorkItemId::try_new("CFG-0015").unwrap(),
                status: Some(WorkItemStatus::Active),
            },
            PrerequisiteStatus {
                id: WorkItemId::try_new("CFG-9999").unwrap(),
                status: None,
            },
        ];

        let output = render_item_for_filter(&item, StatusFilter::default(), true, false);

        assert!(
            output.contains("  prereq: CFG-0014 (done), CFG-0015 (active), CFG-9999 (missing)\n"),
            "{output}"
        );
    }

    #[test]
    fn long_form_omits_effort_line_when_absent() {
        let mut out = String::new();
        render_list_item(
            &mut out,
            &sample_item(),
            StatusFilter::default(),
            true,
            true,
            false,
        );
        assert!(!out.contains("effort:"), "got: {out}");
    }

    #[test]
    fn long_form_shows_raw_tags_line_when_present() {
        let mut item = sample_item();
        item.tags = Some("[SQLite, hand-edited]".to_string());
        let mut out = String::new();
        render_list_item(&mut out, &item, StatusFilter::default(), true, true, false);
        assert!(out.contains("  tags: [SQLite, hand-edited]\n"), "{out}");
    }

    #[test]
    fn long_form_omits_tags_line_when_absent() {
        let mut out = String::new();
        render_list_item(
            &mut out,
            &sample_item(),
            StatusFilter::default(),
            true,
            true,
            false,
        );
        assert!(!out.contains("tags:"), "{out}");
    }

    #[test]
    fn list_footer_mentions_hidden_count_and_escape_hatch() {
        let result = GetPendingWorkOk {
            items: vec![sample_item()],
            hidden: 2,
            project: None,
            project_task_path: None,
            status_filter: StatusFilter::default(),
            grouped: false,
        };
        let output = render_list(
            &result,
            "notes",
            StatusFilter::default(),
            false,
            false,
            false,
        );

        assert!(
            output.ends_with("\n... and 2 more; use '--all' to list everything"),
            "got: {output}"
        );
    }
}
