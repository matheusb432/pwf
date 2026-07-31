use super::{add_pending_work_item, show_pending_work_item};
use crate::project::ProjectRegistry;

pub(in crate::pending_work) mod finding {
    use crate::{
        pending_work::{
            dto::PendingWorkItemView,
            find_pending_work::FindPendingWorkError,
            logic::{enrich, identifier},
        },
        ports::{app_record::AppRecordStore, pending_work_record::PendingWorkRecord},
        project::ProjectRegistry,
    };

    pub(in crate::pending_work) fn find_open_item<S>(
        store: &S,
        projects: &ProjectRegistry,
        id: &str,
    ) -> Result<PendingWorkItemView, FindPendingWorkError>
    where
        S: AppRecordStore<PendingWorkRecord>,
    {
        let requested = id.to_string();
        let Some(work_id) = identifier::parse(id) else {
            return find_inline_open_item(store, projects, &requested);
        };
        let project_id = work_id
            .as_ref()
            .split_once('-')
            .map_or("", |(project_id, _)| project_id);
        if projects.count_projects_by_project_id(project_id) > 1 {
            return Err(FindPendingWorkError::AmbiguousId { id: requested });
        }
        let project = projects.get_project_name_by(&work_id).ok_or_else(|| {
            FindPendingWorkError::UnknownPrefix {
                id: work_id.as_ref().to_string(),
                prefix: project_id.to_string(),
            }
        })?;
        let repo = projects.get_repository_by(project).map(str::to_string);
        let records = store
            .list(project)
            .map_err(|error| FindPendingWorkError::ReadStore(Box::new(error)))?;
        let mut matched: Vec<PendingWorkItemView> = records
            .iter()
            .filter(|record| enrich::is_open_item(record))
            .map(|record| {
                enrich::enrich(record, repo.as_deref())
                    .into_pending_work_item_view(project.as_ref().to_string())
            })
            .filter(|item| item.id == work_id.as_ref())
            .collect();
        match matched.len() {
            0 => Err(FindPendingWorkError::ItemNotFound { id: requested }),
            1 => Ok(matched.pop().expect("length checked")),
            _ => Err(FindPendingWorkError::AmbiguousId { id: requested }),
        }
    }

    fn find_inline_open_item<S>(
        store: &S,
        projects: &ProjectRegistry,
        requested: &str,
    ) -> Result<PendingWorkItemView, FindPendingWorkError>
    where
        S: AppRecordStore<PendingWorkRecord>,
    {
        for (project, repo) in projects.projects() {
            let records = store
                .list(project)
                .map_err(|error| FindPendingWorkError::ReadStore(Box::new(error)))?;
            for record in records {
                if !enrich::is_open_item(&record) {
                    continue;
                }
                let item = enrich::enrich(&record, repo)
                    .into_pending_work_item_view(project.as_ref().to_string());
                if item.format == "legacy" && item.id.eq_ignore_ascii_case(requested) {
                    return Ok(item);
                }
            }
        }
        Err(FindPendingWorkError::ItemNotFound {
            id: requested.to_string(),
        })
    }
}

pub(in crate::pending_work) mod commit_provenance {
    pub(in crate::pending_work) fn normalize(values: &[String]) -> Option<String> {
        let mut ranges = Vec::new();
        for range in values
            .iter()
            .flat_map(|value| value.split(','))
            .map(str::trim)
            .filter(|range| !range.is_empty())
        {
            if !ranges.contains(&range) {
                ranges.push(range);
            }
        }
        (!ranges.is_empty()).then(|| ranges.join(", "))
    }

    #[cfg(test)]
    mod tests {
        use super::normalize;

        #[test]
        fn repeated_raw_ranges_are_normalized_in_first_seen_order() {
            let values = [" a..b,c..d ", "a..b", "", " e..f "]
                .map(str::to_string)
                .to_vec();

            assert_eq!(normalize(&values).as_deref(), Some("a..b, c..d, e..f"));
            assert_eq!(normalize(&[String::new(), "  , ".to_string()]), None);
        }
    }
}

pub(in crate::pending_work) mod enrich {
    //! Derives pending-work launchability diagnostics for list, verify, and session operations.

    use pwf_models::pending_work::WorkItemStatus;

    use super::{note_body::is_placeholder_prompt, section};
    use crate::{
        pending_work::dto::PendingWorkItemView,
        ports::pending_work_record::{Materialization, PendingWorkRecord, RecordId},
    };

    pub(in crate::pending_work) const ISSUE_NO_REPO: &str =
        "Project has no directory source; update the managed project record.";
    pub(in crate::pending_work) const ISSUE_PLACEHOLDER_PROMPT: &str =
        "Prompt is a placeholder; define a real prompt before launching.";

    /// Canonicalizes known section aliases and trims unknown labels for display.
    #[must_use]
    pub(in crate::pending_work) fn normalize_section_label(label: &str) -> String {
        section::alias(label).map_or_else(|| label.trim().to_string(), str::to_string)
    }

    #[must_use]
    pub(in crate::pending_work) fn normalize_section(section: Option<&str>) -> Option<String> {
        section.map(normalize_section_label)
    }

    /// Returns whether an item is active and represented by an open index placement.
    #[must_use]
    pub(in crate::pending_work) fn is_open_item(item: &PendingWorkRecord) -> bool {
        item.status == WorkItemStatus::Active && item.placement.is_some()
    }

    /// Contains launchability flags and diagnostics derived from a pending-work item.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(in crate::pending_work) struct DerivedFlags {
        pub(in crate::pending_work) issues: Vec<String>,
        pub(in crate::pending_work) launchable: bool,
        pub(in crate::pending_work) needs_prompt: bool,
    }

    /// Derives diagnostics in repository, missing-note, placeholder order.
    #[must_use]
    pub(in crate::pending_work) fn derive_flags(
        repo: Option<&str>,
        prompt: &str,
        missing_note: Option<&str>,
    ) -> DerivedFlags {
        let mut issues = Vec::new();
        if repo.is_none_or(|value| value.trim().is_empty()) {
            issues.push(ISSUE_NO_REPO.to_string());
        }
        if let Some(path) = missing_note {
            issues.push(missing_note_issue(path));
        }
        let needs_prompt = is_placeholder_prompt(prompt);
        if needs_prompt {
            issues.push(ISSUE_PLACEHOLDER_PROMPT.to_string());
        }
        DerivedFlags {
            launchable: issues.is_empty(),
            needs_prompt,
            issues,
        }
    }

    /// Contains a pending-work item's persisted data and derived diagnostics before project
    /// attachment.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(in crate::pending_work) struct EnrichedPendingWorkItem {
        pub(in crate::pending_work) id: RecordId,
        pub(in crate::pending_work) status: WorkItemStatus,
        pub(in crate::pending_work) session: String,
        pub(in crate::pending_work) prompt: String,
        pub(in crate::pending_work) repo: Option<String>,
        pub(in crate::pending_work) note: String,
        pub(in crate::pending_work) item_file: Option<String>,
        pub(in crate::pending_work) line: usize,
        pub(in crate::pending_work) format: String,
        pub(in crate::pending_work) launchable: bool,
        pub(in crate::pending_work) needs_prompt: bool,
        pub(in crate::pending_work) issues: Vec<String>,
        pub(in crate::pending_work) section: Option<String>,
        pub(in crate::pending_work) prereq: Option<String>,
        pub(in crate::pending_work) effort: Option<String>,
        pub(in crate::pending_work) tags: Option<String>,
        pub(in crate::pending_work) created: Option<String>,
    }

    impl EnrichedPendingWorkItem {
        /// Attaches the project and composes inline ids as `<project>:<ordinal>`.
        #[must_use]
        pub(in crate::pending_work) fn into_pending_work_item_view(
            self,
            project: String,
        ) -> PendingWorkItemView {
            let id = match &self.id {
                RecordId::Item(id) => id.as_ref().to_string(),
                RecordId::Inline(ordinal) => inline_record_id(&project, *ordinal),
            };
            PendingWorkItemView {
                id,
                project,
                status: self.status,
                session: self.session,
                prompt: self.prompt,
                repo: self.repo,
                note: self.note,
                item_file: self.item_file,
                line: self.line,
                format: self.format,
                launchable: self.launchable,
                needs_prompt: self.needs_prompt,
                issues: self.issues,
                section: self.section,
                prereq: self.prereq,
                prerequisite_statuses: Vec::new(),
                effort: self.effort,
                tags: self.tags,
                created: self.created,
            }
        }
    }

    fn missing_note_issue(path: &str) -> String {
        format!("Work-item note missing: {path}")
    }

    pub(in crate::pending_work) fn inline_record_id(project: &str, ordinal: usize) -> String {
        format!("{project}:{ordinal}")
    }

    /// Projects a persisted item into the fields consumed by list, verify, and session.
    ///
    /// Materialization controls `format` and `item_file`; missing notes add an issue; empty titles
    /// fall back to the canonical id; section labels are canonicalized for display.
    #[must_use]
    pub(in crate::pending_work) fn enrich(
        item: &PendingWorkRecord,
        repo: Option<&str>,
    ) -> EnrichedPendingWorkItem {
        let prompt = item.body.trim().to_string();
        let (format, item_file, missing_note) = match &item.materialization {
            Materialization::NoteFile => ("file", Some(item.locator.clone()), None),
            Materialization::MissingNote { expected } => {
                ("file", Some(item.locator.clone()), Some(expected.as_str()))
            }
            Materialization::InlineLegacy => ("legacy", None, None),
        };
        let flags = derive_flags(repo, &prompt, missing_note);
        let session = match &item.id {
            RecordId::Item(id) if item.title.trim().is_empty() => id.as_ref().to_string(),
            _ => item.title.clone(),
        };
        let (note, line) = item.placement.as_ref().map_or_else(
            || (item.locator.clone(), 1),
            |placement| (placement.index_path.clone(), placement.line),
        );
        EnrichedPendingWorkItem {
            id: item.id.clone(),
            status: item.status,
            session,
            prompt,
            repo: repo.map(str::to_string),
            note,
            line,
            item_file,
            format: format.to_string(),
            launchable: flags.launchable,
            needs_prompt: flags.needs_prompt,
            issues: flags.issues,
            section: normalize_section(item.section.as_deref()),
            prereq: item.prereq.clone(),
            effort: item.effort.clone(),
            tags: item.tags.clone(),
            created: item.created.as_ref().map(|ts| ts.as_str().to_string()),
        }
    }

    #[cfg(test)]
    mod tests {
        use pwf_models::pending_work::{Timestamp, WorkItemId, WorkItemStatus};

        use super::*;
        use crate::ports::pending_work_record::IndexPlacement;

        fn record(body: &str) -> PendingWorkRecord {
            PendingWorkRecord {
                id: RecordId::Item(WorkItemId::try_new("PWF-0001").unwrap()),
                title: "tray gui".to_string(),
                status: WorkItemStatus::Active,
                created: Some(Timestamp::new("2026-01-01".to_string())),
                completed: None,
                commits: None,
                tags: None,
                effort: None,
                prereq: None,
                section: None,
                body: body.to_string(),
                source: String::new(),
                locator: "/notes/pwf/PWF-0001.md".to_string(),
                placement: Some(IndexPlacement {
                    index_path: "/notes/pwf/pwf.md".to_string(),
                    line: 7,
                }),
                materialization: Materialization::NoteFile,
            }
        }

        #[test]
        fn launchable_when_repo_present_and_prompt_real() {
            let enriched = enrich(&record("add startup toggle"), Some("/repo"));
            assert!(enriched.launchable);
            assert!(!enriched.needs_prompt);
            assert!(enriched.issues.is_empty());
            assert_eq!(enriched.prompt, "add startup toggle");
            assert_eq!(enriched.repo.as_deref(), Some("/repo"));
        }

        #[test]
        fn open_item_requires_active_status_and_index_placement() {
            let active = record("body");
            assert!(is_open_item(&active));

            let unlinked = PendingWorkRecord {
                placement: None,
                ..active.clone()
            };
            assert!(!is_open_item(&unlinked));

            let done = PendingWorkRecord {
                status: WorkItemStatus::Done,
                ..active
            };
            assert!(!is_open_item(&done));
        }

        #[test]
        fn note_and_line_render_the_index_placement_not_the_note_file() {
            let enriched = enrich(&record("body"), Some("/repo"));
            assert_eq!(enriched.note, "/notes/pwf/pwf.md");
            assert_eq!(enriched.line, 7);
            assert_eq!(
                enriched.item_file.as_deref(),
                Some("/notes/pwf/PWF-0001.md")
            );
            assert_eq!(enriched.format, "file");
        }

        #[test]
        fn missing_repo_and_placeholder_prompt_are_not_launchable() {
            let enriched = enrich(&record("TODO"), None);
            assert!(!enriched.launchable);
            assert!(enriched.needs_prompt);
            assert_eq!(
                enriched.issues,
                [
                    ISSUE_NO_REPO.to_string(),
                    ISSUE_PLACEHOLDER_PROMPT.to_string()
                ]
            );
        }

        #[test]
        fn missing_note_wikilink_needs_attention_with_missing_note_issue() {
            let mut rec = record("");
            rec.materialization = Materialization::MissingNote {
                expected: "/notes/pwf/PWF-0001.md".to_string(),
            };

            let enriched = enrich(&rec, Some("/repo"));

            assert!(!enriched.launchable, "missing note must not be launchable");
            assert!(enriched.needs_prompt, "empty prompt is a placeholder");
            assert_eq!(
                enriched.issues,
                [
                    "Work-item note missing: /notes/pwf/PWF-0001.md".to_string(),
                    ISSUE_PLACEHOLDER_PROMPT.to_string(),
                ]
            );
            assert_eq!(enriched.prompt, "");
            assert_eq!(enriched.format, "file");
            assert_eq!(
                enriched.item_file.as_deref(),
                Some("/notes/pwf/PWF-0001.md")
            );
        }

        #[test]
        fn inline_legacy_record_projects_legacy_format_and_ordinal_id() {
            let inline = PendingWorkRecord {
                id: RecordId::Inline(2),
                title: "legacy task".to_string(),
                body: "do the legacy thing".to_string(),
                source: "do the legacy thing".to_string(),
                locator: "/notes/pwf/pwf.md".to_string(),
                materialization: Materialization::InlineLegacy,
                ..record("")
            };

            let item =
                enrich(&inline, Some("/repo")).into_pending_work_item_view("pwf".to_string());

            assert_eq!(item.id, "pwf:2");
            assert_eq!(item.format, "legacy");
            assert_eq!(item.item_file, None);
            assert_eq!(item.session, "legacy task");
            assert_eq!(item.prompt, "do the legacy thing");
            assert!(item.launchable);
        }

        #[test]
        fn empty_title_falls_back_to_id() {
            let mut rec = record("body");
            rec.title = "  ".to_string();
            assert_eq!(enrich(&rec, Some("/repo")).session, "PWF-0001");
        }

        #[test]
        fn section_label_is_normalized() {
            let mut rec = record("body");
            rec.section = Some("Futuro".to_string());
            assert_eq!(
                enrich(&rec, Some("/repo")).section.as_deref(),
                Some("Future")
            );
        }

        #[test]
        fn unknown_section_label_preserves_trimmed_case() {
            assert_eq!(normalize_section_label(" SomeDay "), "SomeDay");
        }

        #[test]
        fn derive_flags_orders_issues_no_repo_missing_note_placeholder() {
            let flags = derive_flags(None, "", Some("/notes/pwf/PWF-0009.md"));
            assert_eq!(
                flags.issues,
                [
                    ISSUE_NO_REPO.to_string(),
                    "Work-item note missing: /notes/pwf/PWF-0009.md".to_string(),
                    ISSUE_PLACEHOLDER_PROMPT.to_string(),
                ]
            );
        }
    }
}

pub(in crate::pending_work) mod identifier {
    use pwf_models::pending_work::WorkItemId;

    pub(in crate::pending_work) fn normalize(raw: &str) -> String {
        let trimmed = raw.trim().to_ascii_uppercase();
        match split_compact(&trimmed) {
            Some((code, number)) => format!("{code}-{number:04}"),
            None => trimmed,
        }
    }

    pub(in crate::pending_work) fn parse(raw: &str) -> Option<WorkItemId> {
        WorkItemId::try_new(normalize(raw)).ok()
    }

    fn split_compact(raw: &str) -> Option<(&str, u32)> {
        let digit_start = raw.find(|character: char| character.is_ascii_digit())?;
        let (code, digits) = raw.split_at(digit_start);
        let code = code.strip_suffix('-').unwrap_or(code);
        if !(2..=4).contains(&code.len())
            || !code.chars().all(|character| character.is_ascii_uppercase())
        {
            return None;
        }
        if digits.is_empty()
            || digits.len() > 4
            || !digits.chars().all(|character| character.is_ascii_digit())
        {
            return None;
        }
        Some((code, digits.parse().ok()?))
    }

    #[cfg(test)]
    mod tests {
        use super::normalize;

        #[test]
        fn loose_identifiers_normalize_all_accepted_forms() {
            assert_eq!(normalize("PWF-0098"), "PWF-0098");
            assert_eq!(normalize("pwf-0098"), "PWF-0098");
            assert_eq!(normalize("  pwf-0047  "), "PWF-0047");
            assert_eq!(normalize("pwf-98"), "PWF-0098");
            assert_eq!(normalize("cfg57"), "CFG-0057");
            assert_eq!(normalize("CFG57"), "CFG-0057");
            assert_eq!(normalize("cfg-57"), "CFG-0057");
            assert_eq!(normalize("garbage"), "GARBAGE");
            assert_eq!(normalize("PWF-0047-extra"), "PWF-0047-EXTRA");
            assert_eq!(normalize("toolong-0047"), "TOOLONG-0047");
        }
    }
}

pub(in crate::pending_work) mod note_body {
    //! Applies prompt, lane, and report transforms to pending-work note bodies.

    use std::sync::LazyLock;

    use prompt_lanes::{Adapter, MarkdownAdapter, parse};
    use regex::Regex;

    const REPORT_HEADER: &str = "### Report";
    const LANE_SECTION_HEADERS: [&str; 4] =
        ["## Goals", "## Context", "## Constraints", "## Done When"];

    static PLACEHOLDER_PROMPT_REGEX: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)(^\s*\[!\]\s*TODO\b|^\s*TODO\b|definir prompt|define prompt|tbd)")
            .expect("valid placeholder regex")
    });

    enum PromptClassification {
        Placeholder,
        AuthoredVerbatimLegacy,
        Authored,
    }

    #[must_use]
    pub(in crate::pending_work) fn render(prompt: &str) -> String {
        match prompt_classification(prompt) {
            PromptClassification::Placeholder | PromptClassification::AuthoredVerbatimLegacy => {
                prompt.to_string()
            }
            PromptClassification::Authored => MarkdownAdapter.render(&parse(prompt)),
        }
    }

    /// Reports whether a prompt is empty or matches `TODO`, `[!] TODO`, `define prompt`, `definir
    /// prompt`, or `tbd` case-insensitively.
    #[must_use]
    pub(in crate::pending_work) fn is_placeholder_prompt(prompt: &str) -> bool {
        matches!(
            prompt_classification(prompt),
            PromptClassification::Placeholder
        )
    }

    fn prompt_classification(prompt: &str) -> PromptClassification {
        if prompt.trim().is_empty() || PLACEHOLDER_PROMPT_REGEX.is_match(prompt) {
            return PromptClassification::Placeholder;
        }

        // Rendering uses ASCII boundaries, but diagnostics use Unicode boundaries.
        let prompt_lowercase = prompt.to_lowercase();
        let prompt_trimmed = prompt_lowercase.trim_start();
        let prompt_after_marker = prompt_trimmed.strip_prefix("[!]").map(str::trim_start);
        if [Some(prompt_trimmed), prompt_after_marker]
            .into_iter()
            .flatten()
            .any(prompt_starts_with_todo_word_boundary_ascii)
        {
            PromptClassification::AuthoredVerbatimLegacy
        } else {
            PromptClassification::Authored
        }
    }

    fn prompt_starts_with_todo_word_boundary_ascii(prompt: &str) -> bool {
        prompt.strip_prefix("todo").is_some_and(|rest| {
            !rest.starts_with(|character: char| {
                character.is_ascii_alphanumeric() || character == '_'
            })
        })
    }

    /// Splices lane bullets into existing sections and appends missing sections.
    ///
    /// Returns [`None`] for a whitespace-only prompt.
    #[must_use]
    pub(in crate::pending_work) fn append_lanes(body: &str, prompt: &str) -> Option<String> {
        let prompt = prompt.trim();
        if prompt.is_empty() {
            return None;
        }
        let mut parsed = parse(prompt);
        if !parsed.title.is_empty() {
            parsed.goals.insert(0, std::mem::take(&mut parsed.title));
        }
        let sections: [&[String]; 4] = [
            &parsed.goals,
            &parsed.context,
            &parsed.constraints,
            &parsed.done_when,
        ];
        let mut out = body.to_string();
        for (header, bullets) in LANE_SECTION_HEADERS.iter().zip(sections) {
            out = append_bullets_to_section(&out, header, bullets);
        }
        Some(out)
    }

    fn append_bullets_to_section(content: &str, header: &str, bullets: &[String]) -> String {
        if bullets.is_empty() {
            return content.to_string();
        }
        let Some(header_end) = header_line_end(content, header) else {
            let mut out = content.trim_end().to_string();
            out.push_str("\n\n");
            out.push_str(header);
            for bullet in bullets {
                out.push_str("\n- ");
                out.push_str(bullet);
            }
            out.push('\n');
            return out;
        };
        let rest = &content[header_end..];
        let section_end = header_end + next_heading_offset(rest).unwrap_or(rest.len());
        let before = content[..section_end].trim_end_matches('\n');
        let after = content[section_end..].trim_start_matches('\n');
        let mut out = before.to_string();
        for bullet in bullets {
            out.push_str("\n- ");
            out.push_str(bullet);
        }
        if after.is_empty() {
            out.push('\n');
        } else {
            out.push_str("\n\n");
            out.push_str(after);
        }
        out
    }

    /// Returns the byte offset after an exact header line, allowing trailing whitespace.
    fn header_line_end(content: &str, header: &str) -> Option<usize> {
        let mut offset = 0;
        for segment in content.split('\n') {
            if segment
                .strip_prefix(header)
                .is_some_and(|rest| rest.trim().is_empty())
            {
                return Some(offset + segment.len());
            }
            offset += segment.len() + 1;
        }
        None
    }

    /// Returns the byte offset of the first Markdown heading line.
    fn next_heading_offset(content: &str) -> Option<usize> {
        let mut offset = 0;
        for segment in content.split('\n') {
            if is_heading_line(segment) {
                return Some(offset);
            }
            offset += segment.len() + 1;
        }
        None
    }

    fn is_heading_line(line: &str) -> bool {
        let hashes = line.bytes().take_while(|&b| b == b'#').count();
        (1..=6).contains(&hashes) && line[hashes..].starts_with(char::is_whitespace)
    }

    /// Appends a free-form Markdown report verbatim under one `### Report` heading.
    ///
    /// Returns [`None`] for a whitespace-only report.
    #[must_use]
    pub(in crate::pending_work) fn append_report_block(body: &str, report: &str) -> Option<String> {
        let report = report.trim();
        if report.is_empty() {
            return None;
        }
        let mut out = body.trim_end().to_string();
        if !has_report_header(&out) {
            out.push_str("\n\n");
            out.push_str(REPORT_HEADER);
        }
        out.push_str("\n\n");
        out.push_str(report);
        out.push('\n');
        Some(out)
    }

    fn has_report_header(content: &str) -> bool {
        content.lines().any(|line| {
            line.strip_prefix(REPORT_HEADER)
                .is_some_and(|rest| rest.trim().is_empty())
        })
    }

    /// Appends a whitespace-collapsed report under a new `### Report` heading.
    ///
    /// Returns [`None`] for a blank report.
    #[must_use]
    pub(in crate::pending_work) fn append_report(body: &str, report: &str) -> Option<String> {
        let report = normalized_report(report)?;
        let mut out = body.trim_end().to_string();
        out.push_str("\n\n");
        out.push_str(REPORT_HEADER);
        out.push_str("\n\n");
        out.push_str(&report);
        out.push('\n');
        Some(out)
    }

    fn normalized_report(report: &str) -> Option<String> {
        let mut parts = Vec::new();
        for line in report.lines() {
            let line = line.trim();
            if !line.is_empty() {
                parts.push(line);
            }
        }
        (!parts.is_empty()).then(|| parts.join(" "))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        const S: &str = "\n\n";

        #[test]
        fn placeholder_prompt_detection() {
            assert!(is_placeholder_prompt(""));
            assert!(is_placeholder_prompt("TODO define this"));
            assert!(is_placeholder_prompt("definir prompt"));
            assert!(!is_placeholder_prompt("add startup toggle"));
        }

        #[test]
        fn note_body_renders_marker_first_prompt_without_body_leakage() {
            assert_eq!(
                render("/c context"),
                format!("## Goals\n{S}## Context{S}- context")
            );
        }

        #[test]
        fn note_body_wraps_a_normal_prompt() {
            assert_eq!(render("add startup toggle"), "## Goals\n");
            assert_eq!(render("a / b"), format!("## Goals{S}- b"));
        }

        #[test]
        fn note_body_renders_one_bullet_per_slash_lane() {
            assert_eq!(
                render("create engine feature to add update task / make it idempotent"),
                format!("## Goals{S}- make it idempotent")
            );
        }

        #[test]
        fn note_body_preserves_ampersands_as_text() {
            assert_eq!(render("a & b"), "## Goals\n");
        }

        #[test]
        fn note_body_keeps_placeholder_raw_so_it_stays_detectable() {
            assert_eq!(render("TODO"), "TODO");
            assert!(is_placeholder_prompt(&render("TODO")));
            assert!(is_placeholder_prompt(&render("tbd")));
            assert!(is_placeholder_prompt(&render("define prompt")));
            assert_eq!(render("tbd"), "tbd");
            assert_eq!(render("define prompt"), "define prompt");
        }

        #[test]
        fn placeholder_detection_matches_the_legacy_regex_cases() {
            for raw in [
                "",
                "   ",
                "TODO",
                "todo",
                "TODO: implement",
                "[!] TODO",
                "[!]TODO",
                "tbd",
                "TBD later",
                "define prompt",
                "definir prompt",
                "the plan is tbd",
            ] {
                assert!(is_placeholder_prompt(raw), "should be placeholder: {raw:?}");
            }
            for raw in [
                "add startup toggle",
                "todolist cleanup",
                "a / b",
                "/c context",
            ] {
                assert!(
                    !is_placeholder_prompt(raw),
                    "should not be placeholder: {raw:?}"
                );
            }
        }

        #[test]
        fn unicode_todo_suffix_remains_raw_without_becoming_placeholder() {
            for prompt in ["TODOé", "[!] TODOé"] {
                assert!(!is_placeholder_prompt(prompt));
                assert_eq!(render(prompt), prompt);
            }
            assert!(is_placeholder_prompt("TODO-implement"));
        }

        #[test]
        fn append_lanes_grows_an_existing_section_in_place() {
            assert_eq!(
                append_lanes("## Goals\n- do the thing\n", "also this").unwrap(),
                "## Goals\n- do the thing\n- also this\n"
            );
        }

        #[test]
        fn append_lanes_creates_a_missing_section_at_the_end() {
            assert_eq!(
                append_lanes("## Goals\n- do the thing\n", "another goal /c new context").unwrap(),
                "## Goals\n- do the thing\n- another goal\n\n## Context\n- new context\n"
            );
        }

        #[test]
        fn append_lanes_marker_first_leaves_goals_untouched() {
            assert_eq!(
                append_lanes("## Goals\n- do the thing\n", "/c context").unwrap(),
                "## Goals\n- do the thing\n\n## Context\n- context\n"
            );
        }

        #[test]
        fn append_lanes_rejects_whitespace_only() {
            assert_eq!(append_lanes("## Goals\n- x\n", "   \n\t"), None);
        }

        #[test]
        fn append_report_block_appends_verbatim_and_reuses_an_existing_header() {
            let body = "## Goals\n\n- ship it\n";
            let report = "## Outcome\n\nShipped it.\n\n## Follow-ups\n\n- write the release notes";
            assert_eq!(
                append_report_block(body, report).unwrap(),
                "## Goals\n\n- ship it\n\n### Report\n\n## Outcome\n\nShipped it.\n\n## Follow-ups\n\n- write the release notes\n"
            );
            let once = append_report_block(body, "first").unwrap();
            let twice = append_report_block(&once, "second").unwrap();
            assert_eq!(twice.matches("### Report").count(), 1, "{twice}");
        }

        #[test]
        fn append_report_block_rejects_whitespace_only() {
            assert_eq!(append_report_block("body\n", "   \n\t"), None);
        }

        #[test]
        fn append_report_collapses_multiline_into_a_single_line_block() {
            assert_eq!(
                append_report("body\n", "line one\n\nline two").unwrap(),
                "body\n\n### Report\n\nline one line two\n"
            );
            assert_eq!(append_report("body\n", "  \n\t"), None);
        }
    }
}

pub(in crate::pending_work) mod prerequisite {
    use std::sync::LazyLock;

    use pwf_models::pending_work::{WorkItemId, WorkItemStatus};
    use regex::Regex;

    use super::{ProjectRegistry, identifier};
    use crate::{
        pending_work::dto::PrerequisiteStatus,
        ports::{
            app_record::AppRecordStore,
            pending_work_record::{Materialization, PendingWorkRecord},
        },
    };

    const PREREQUISITE_VALUE_PATTERN: &str = r"\[\[([A-Z]{2,4}-\d{4})";
    static PREREQUISITE_VALUE_REGEX: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(PREREQUISITE_VALUE_PATTERN).expect("valid prerequisite value regex")
    });
    static PERSISTED_STATUS_REGEX: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?m)^status:\s*([^\r\n]+)$").expect("valid persisted status regex")
    });
    static PERSISTED_FRONTMATTER_REGEX: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?s)\A(?:\u{feff})?---[ \t]*\r?\n(.*?)\r?\n---[ \t]*(?:\r?\n|\z)")
            .expect("valid persisted frontmatter regex")
    });

    #[derive(Debug, thiserror::Error)]
    pub(in crate::pending_work) enum PrerequisiteValidationError {
        #[error("Invalid --prereq id: {raw}.")]
        InvalidId { raw: String },
        #[error("--prereq requires an id.")]
        MissingId,
        #[error("Unknown --prereq id(s): {}.", ids.join(", "))]
        UnknownIds { ids: Vec<String> },
    }

    fn parse_values(values: &[String]) -> Result<Vec<WorkItemId>, PrerequisiteValidationError> {
        let mut identifiers = Vec::new();
        for value in values {
            for raw in value.split(',') {
                let raw = raw.trim();
                if raw.is_empty() {
                    continue;
                }
                let candidate = raw
                    .strip_prefix("[[")
                    .and_then(|trimmed| trimmed.strip_suffix("]]"))
                    .unwrap_or(raw);
                let identifier = identifier::parse(candidate).ok_or_else(|| {
                    PrerequisiteValidationError::InvalidId {
                        raw: raw.to_string(),
                    }
                })?;
                if !identifiers.contains(&identifier) {
                    identifiers.push(identifier);
                }
            }
        }
        if identifiers.is_empty() {
            return Err(PrerequisiteValidationError::MissingId);
        }
        Ok(identifiers)
    }

    pub(in crate::pending_work) fn parse_frontmatter(
        raw: &str,
    ) -> Result<Vec<WorkItemId>, PrerequisiteValidationError> {
        let raw = raw.trim();
        let unquoted = raw
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
            .or_else(|| {
                raw.strip_prefix('\'')
                    .and_then(|value| value.strip_suffix('\''))
            })
            .unwrap_or(raw);
        parse_values(&[unquoted.to_string()])
    }

    fn frontmatter_value(identifiers: &[WorkItemId]) -> String {
        identifiers
            .iter()
            .map(|identifier| format!("[[{identifier}]]"))
            .collect::<Vec<_>>()
            .join(", ")
    }

    pub(in crate::pending_work) fn validate_and_merge<S>(
        existing: Option<&str>,
        values: &[String],
        store: &S,
        projects: &ProjectRegistry,
    ) -> Result<String, PrerequisiteValidationError>
    where
        S: AppRecordStore<PendingWorkRecord>,
    {
        let prerequisites = parse_values(values)?;
        let mut unknown = Vec::new();
        for identifier in &prerequisites {
            let Some(project) = projects.get_project_name_by(identifier) else {
                unknown.push(identifier.as_ref().to_string());
                continue;
            };
            let Ok(record) = store.get(project, identifier) else {
                unknown.push(identifier.as_ref().to_string());
                continue;
            };
            if record.is_none_or(|record| !has_valid_persisted_status(&record)) {
                unknown.push(identifier.as_ref().to_string());
            }
        }
        if !unknown.is_empty() {
            return Err(PrerequisiteValidationError::UnknownIds { ids: unknown });
        }

        let mut identifiers = Vec::new();
        for identifier in existing
            .into_iter()
            .flat_map(|value| PREREQUISITE_VALUE_REGEX.captures_iter(value))
            .map(|captures| {
                WorkItemId::try_new(&captures[1])
                    .expect("prerequisite regex captures a canonical work-item id")
            })
            .chain(prerequisites)
        {
            if !identifiers.contains(&identifier) {
                identifiers.push(identifier);
            }
        }
        Ok(frontmatter_value(&identifiers))
    }

    fn has_valid_persisted_status(record: &PendingWorkRecord) -> bool {
        matches!(record.materialization, Materialization::NoteFile)
            && PERSISTED_FRONTMATTER_REGEX
                .captures(&record.source)
                .and_then(|captures| captures.get(1))
                .and_then(|frontmatter| PERSISTED_STATUS_REGEX.captures(frontmatter.as_str()))
                .and_then(|captures| captures.get(1))
                .is_some_and(|value| value.as_str().trim().parse::<WorkItemStatus>().is_ok())
    }

    pub(in crate::pending_work) fn statuses(
        value: &str,
        store: &impl AppRecordStore<PendingWorkRecord>,
        projects: &ProjectRegistry,
    ) -> Vec<PrerequisiteStatus> {
        PREREQUISITE_VALUE_REGEX
            .captures_iter(value)
            .map(|captures| {
                let id = WorkItemId::try_new(&captures[1])
                    .expect("prerequisite regex captures a canonical work-item id");
                let status = status(store, projects, &id);
                PrerequisiteStatus { id, status }
            })
            .collect()
    }

    #[rustfmt::skip]
    fn status(
        store: &impl AppRecordStore<PendingWorkRecord>,
        projects: &ProjectRegistry,
        id: &WorkItemId,
    ) -> Option<WorkItemStatus> {
        let project = projects.get_project_name_by(id)?;
        // FIXME: Distinguish an absent prerequisite from a read or parse failure; both currently render as "missing" and can hide vault corruption.
        store
            .get(project, id)
            .ok()
            .flatten()
            .filter(|item| !matches!(&item.materialization, Materialization::MissingNote { .. }))
            .map(|item| item.status)
    }

    #[cfg(test)]
    mod tests {
        use pwf_models::pending_work::{ProjectName, WorkItemId};

        use super::{
            PrerequisiteValidationError, frontmatter_value, parse_values, validate_and_merge,
        };
        use crate::{
            pending_work::resolve::testing::{staged, staged_ghost},
            ports::{
                app_record::AppRecordStore,
                pending_work_record::{ItemPatch, NewItem, PendingWorkRecord},
            },
        };

        #[derive(Clone)]
        struct ReadFailureStore;

        #[derive(Debug, thiserror::Error)]
        #[error("read failed")]
        struct ReadFailure;

        impl AppRecordStore<PendingWorkRecord> for ReadFailureStore {
            type Error = ReadFailure;

            fn get(
                &self,
                _scope: &ProjectName,
                _id: &WorkItemId,
            ) -> Result<Option<PendingWorkRecord>, Self::Error> {
                Err(ReadFailure)
            }

            fn list(&self, _scope: &ProjectName) -> Result<Vec<PendingWorkRecord>, Self::Error> {
                unreachable!("validator reads one prerequisite")
            }

            fn insert(
                &self,
                _scope: &ProjectName,
                _new: NewItem,
            ) -> Result<PendingWorkRecord, Self::Error> {
                unreachable!("validator is read-only")
            }

            fn update(
                &self,
                _scope: &ProjectName,
                _id: &WorkItemId,
                _patch: ItemPatch,
            ) -> Result<(), Self::Error> {
                unreachable!("validator is read-only")
            }

            fn delete(&self, _scope: &ProjectName, _id: &WorkItemId) -> Result<(), Self::Error> {
                unreachable!("validator is read-only")
            }
        }

        #[test]
        fn loose_values_normalize_deduplicate_and_render() {
            let prerequisites =
                parse_values(&["cfg57, [[CFG-0014]]".to_string(), "CFG-14".to_string()]).unwrap();

            assert_eq!(
                prerequisites.iter().map(AsRef::as_ref).collect::<Vec<_>>(),
                ["CFG-0057", "CFG-0014"]
            );
            assert_eq!(
                frontmatter_value(&prerequisites),
                "[[CFG-0057]], [[CFG-0014]]"
            );
        }

        #[test]
        fn loose_values_reject_empty_input() {
            assert!(parse_values(&[]).is_err());
            assert!(parse_values(&[" , ".to_string()]).is_err());
        }

        #[test]
        fn validator_parses_checks_existence_and_merges_first_seen_ids() {
            let (store, projects) = staged();
            let merged = validate_and_merge(
                Some("[[PWF-0001]], [[PWF-0001]]"),
                &["pwf1, PWF-0001".to_string()],
                &store,
                &projects,
            )
            .unwrap();

            assert_eq!(merged, "[[PWF-0001]]");
        }

        #[test]
        fn validator_preserves_prerequisite_diagnostics() {
            let (store, projects) = staged();

            let invalid = validate_and_merge(None, &["PWF-99999".to_string()], &store, &projects)
                .unwrap_err();
            assert!(matches!(
                invalid,
                PrerequisiteValidationError::InvalidId { ref raw } if raw == "PWF-99999"
            ));
            assert_eq!(invalid.to_string(), "Invalid --prereq id: PWF-99999.");

            let missing =
                validate_and_merge(None, &[", ,".to_string()], &store, &projects).unwrap_err();
            assert!(matches!(missing, PrerequisiteValidationError::MissingId));
            assert_eq!(missing.to_string(), "--prereq requires an id.");

            let unknown =
                validate_and_merge(None, &["PWF-9999".to_string()], &store, &projects).unwrap_err();
            assert!(matches!(
                unknown,
                PrerequisiteValidationError::UnknownIds { ref ids }
                    if ids == &vec!["PWF-9999".to_string()]
            ));
            assert_eq!(unknown.to_string(), "Unknown --prereq id(s): PWF-9999.");
        }

        #[test]
        fn validator_rejects_missing_note_materializations_as_unknown() {
            let (store, projects) = staged_ghost();

            let error =
                validate_and_merge(None, &["PWF-0002".to_string()], &store, &projects).unwrap_err();

            assert!(matches!(
                error,
                PrerequisiteValidationError::UnknownIds { ref ids }
                    if ids == &vec!["PWF-0002".to_string()]
            ));
        }

        #[test]
        fn validator_classifies_store_read_failures_as_unknown_prerequisites() {
            let (_store, projects) = staged();

            let error = validate_and_merge(
                None,
                &["PWF-0001".to_string()],
                &ReadFailureStore,
                &projects,
            )
            .unwrap_err();

            assert!(matches!(
                error,
                PrerequisiteValidationError::UnknownIds { ref ids }
                    if ids == &vec!["PWF-0001".to_string()]
            ));
            assert_eq!(error.to_string(), "Unknown --prereq id(s): PWF-0001.");
        }

        #[test]
        fn validator_rejects_missing_or_invalid_persisted_status_as_unknown() {
            let (staged_store, projects) = staged();
            let base = staged_store.items("pwf")[0].clone();
            for source in [
                "---\nid: PWF-0001\ntitle: task\n---\n\nbody\n",
                "---\nid: PWF-0001\nstatus: paused\ntitle: task\n---\n\nbody\n",
                "---\nid: PWF-0001\ntitle: task\n---\n\nstatus: active\n",
            ] {
                let record = PendingWorkRecord {
                    source: source.to_string(),
                    ..base.clone()
                };
                let store =
                    crate::testing::InMemoryStore::default().with_project("pwf", vec![record]);

                let error = validate_and_merge(None, &["PWF-0001".to_string()], &store, &projects)
                    .unwrap_err();

                assert!(matches!(
                    error,
                    PrerequisiteValidationError::UnknownIds { ref ids }
                        if ids == &vec!["PWF-0001".to_string()]
                ));
            }
        }
    }
}

pub(in crate::pending_work) mod resolve {
    use pwf_models::pending_work::ProjectName;

    use super::{
        ProjectRegistry, enrich::inline_record_id, identifier,
        show_pending_work_item::ShowPendingWorkError,
    };
    use crate::ports::{
        app_record::AppRecordStore,
        pending_work_record::{PendingWorkRecord, RecordId},
    };

    /// Resolves an open or closed record by project ID or inline `<project>:<ordinal>` id.
    pub(in crate::pending_work) fn resolve_record<S>(
        store: &S,
        projects: &ProjectRegistry,
        id: &str,
    ) -> Result<(ProjectName, PendingWorkRecord), ShowPendingWorkError>
    where
        S: AppRecordStore<PendingWorkRecord>,
    {
        let not_found = || ShowPendingWorkError::ItemNotFound { id: id.to_string() };
        let Some(work_id) = identifier::parse(id) else {
            return resolve_inline_record(store, projects, id);
        };
        let project = projects
            .get_project_name_by(&work_id)
            .ok_or_else(not_found)?;
        store
            .get(project, &work_id)
            .map_err(|error| ShowPendingWorkError::ReadStore(Box::new(error)))?
            .map(|record| (project.clone(), record))
            .ok_or_else(not_found)
    }

    /// Finds an inline `<project>:<ordinal>` id case-insensitively across managed projects.
    fn resolve_inline_record<S>(
        store: &S,
        projects: &ProjectRegistry,
        id: &str,
    ) -> Result<(ProjectName, PendingWorkRecord), ShowPendingWorkError>
    where
        S: AppRecordStore<PendingWorkRecord>,
    {
        for (project, _repo) in projects.projects() {
            let records = store
                .list(project)
                .map_err(|error| ShowPendingWorkError::ReadStore(Box::new(error)))?;
            let found = records.into_iter().find(|record| match record.id {
                RecordId::Inline(ordinal) => {
                    inline_record_id(project.as_ref(), ordinal).eq_ignore_ascii_case(id)
                }
                RecordId::Item(_) => false,
            });
            if let Some(record) = found {
                return Ok((project.clone(), record));
            }
        }
        Err(ShowPendingWorkError::ItemNotFound { id: id.to_string() })
    }

    #[cfg(test)]
    pub(in crate::pending_work) mod testing {
        use pwf_models::pending_work::{ProjectName, Timestamp, WorkItemId, WorkItemStatus};

        use super::ProjectRegistry;
        use crate::{
            ports::pending_work_record::{Materialization, PendingWorkRecord, RecordId},
            testing::InMemoryStore,
        };

        pub(in crate::pending_work) const PWF_0001_SOURCE: &str = "---\nid: PWF-0001\nstatus: active\ntitle: do the thing\nproject: pwf\ncreated: 2026-06-20\n---\n\n## Goals\n- do the thing\n";

        pub(in crate::pending_work) fn staged() -> (InMemoryStore, ProjectRegistry) {
            let record = PendingWorkRecord {
                id: RecordId::Item(WorkItemId::try_new("PWF-0001").unwrap()),
                title: "do the thing".to_string(),
                status: WorkItemStatus::Active,
                created: Some(Timestamp::new("2026-06-20")),
                completed: None,
                commits: None,
                tags: None,
                effort: None,
                prereq: None,
                section: None,
                body: "\n## Goals\n- do the thing\n".to_string(),
                source: PWF_0001_SOURCE.to_string(),
                locator: "/notes/pwf/PWF-0001.md".to_string(),
                placement: None,
                materialization: Materialization::NoteFile,
            };
            let store = InMemoryStore::default().with_project("pwf", vec![record]);
            (store, registry())
        }

        pub(in crate::pending_work) fn staged_ghost() -> (InMemoryStore, ProjectRegistry) {
            let record = PendingWorkRecord {
                id: RecordId::Item(WorkItemId::try_new("PWF-0002").unwrap()),
                title: "ghost".to_string(),
                status: WorkItemStatus::Active,
                created: None,
                completed: None,
                commits: None,
                tags: None,
                effort: None,
                prereq: None,
                section: None,
                body: String::new(),
                source: String::new(),
                locator: "/notes/pwf/PWF-0002.md".to_string(),
                placement: None,
                materialization: Materialization::MissingNote {
                    expected: "/notes/pwf/PWF-0002.md".to_string(),
                },
            };
            let store = InMemoryStore::default().with_project("pwf", vec![record]);
            (store, registry())
        }

        pub(in crate::pending_work) fn staged_inline() -> (InMemoryStore, ProjectRegistry) {
            let record = PendingWorkRecord {
                id: RecordId::Inline(1),
                title: "legacy task".to_string(),
                status: WorkItemStatus::Active,
                created: None,
                completed: None,
                commits: None,
                tags: None,
                effort: None,
                prereq: None,
                section: None,
                body: "do the legacy thing".to_string(),
                source: "do the legacy thing".to_string(),
                locator: "/notes/pwf/pwf.md".to_string(),
                placement: None,
                materialization: Materialization::InlineLegacy,
            };
            let store = InMemoryStore::default().with_project("pwf", vec![record]);
            (store, registry())
        }

        fn registry() -> ProjectRegistry {
            ProjectRegistry::new(vec![(
                ProjectName::try_new("pwf").unwrap(),
                Some("/repo/pwf".to_string()),
                Some("PWF".to_string()),
            )])
        }
    }

    #[cfg(test)]
    mod tests {
        use super::{
            ShowPendingWorkError, resolve_record,
            testing::{staged, staged_ghost, staged_inline},
        };

        #[test]
        fn resolve_record_returns_path_and_markdown() {
            let (store, registry) = staged();

            let (project, resolved) = resolve_record(&store, &registry, "PWF-0001").unwrap();

            assert_eq!(project.as_ref(), "pwf");
            assert_eq!(resolved.locator, "/notes/pwf/PWF-0001.md");
            assert_eq!(resolved.source, super::testing::PWF_0001_SOURCE);
        }

        #[test]
        fn resolve_returns_expected_note_path_for_missing_note_wikilink() {
            let (store, registry) = staged_ghost();

            let (_project, resolved) = resolve_record(&store, &registry, "PWF-0002").unwrap();

            assert_eq!(resolved.locator, "/notes/pwf/PWF-0002.md");
        }

        #[test]
        fn resolve_serves_inline_legacy_id_case_insensitively() {
            let (store, registry) = staged_inline();

            let (project, resolved) = resolve_record(&store, &registry, "PWF:1").unwrap();

            assert_eq!(project.as_ref(), "pwf");
            assert_eq!(resolved.locator, "/notes/pwf/pwf.md");
            assert_eq!(resolved.source, "do the legacy thing");
        }

        #[test]
        fn resolve_unknown_inline_id_preserves_raw_id() {
            let (store, registry) = staged_inline();

            let error = resolve_record(&store, &registry, "pwf:9").unwrap_err();

            assert_eq!(error.to_string(), "Open pending-work item not found: pwf:9");
        }

        #[test]
        fn resolve_missing_id_preserves_raw_lowercase_id() {
            let (store, registry) = staged();

            let error = resolve_record(&store, &registry, "pwf-9999").unwrap_err();

            assert_eq!(
                error.to_string(),
                "Open pending-work item not found: pwf-9999"
            );
            assert!(matches!(
                error,
                ShowPendingWorkError::ItemNotFound { id } if id == "pwf-9999"
            ));
        }
    }
}

pub(in crate::pending_work) mod section {
    #[must_use]
    pub(in crate::pending_work) fn alias(label: &str) -> Option<&'static str> {
        match label.trim().to_ascii_lowercase().as_str() {
            "future" | "futuro" => Some("Future"),
            "human" => Some("Human"),
            "low-prio" | "low-priority" => Some("Low-prio"),
            _ => None,
        }
    }

    #[cfg(test)]
    mod tests {
        use super::alias;

        #[test]
        fn aliases_map_only_known_labels() {
            assert_eq!(alias(" Futuro "), Some("Future"));
            assert_eq!(alias("future"), Some("Future"));
            assert_eq!(alias("HUMAN"), Some("Human"));
            assert_eq!(alias("low-priority"), Some("Low-prio"));
            assert_eq!(alias("Someday"), None);
        }
    }
}

pub(in crate::pending_work) mod store_util {
    //! Shared item loading and creation over the persistence ports.

    use pwf_models::pending_work::{ProjectName, WorkItemId};

    use super::{add_pending_work_item::CreateItemError, enrich::normalize_section_label};
    use crate::ports::{
        app_record::AppRecordStore,
        pending_work_record::{
            IndexEntry, IndexEntryState, IndexSection, NewItem, PendingWorkRecord,
        },
    };

    /// Reports failures while loading a required item.
    #[derive(Debug, thiserror::Error)]
    pub(in crate::pending_work) enum LoadItemError {
        #[error("Open pending-work item not found: {id}")]
        ItemNotFound { id: String },
        #[error("{0}")]
        Store(Box<dyn std::error::Error + Send + Sync>),
    }

    /// Canonical labels that materialize as dedicated H2 sections.
    const SECTION_LABELS: [&str; 3] = ["Future", "Human", "Low-prio"];

    /// Contains a created record and the new H2 section, if one was needed.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(in crate::pending_work) struct CreatedItem {
        pub(in crate::pending_work) record: PendingWorkRecord,
        pub(in crate::pending_work) created_section: Option<String>,
    }

    /// Strips the frontmatter parser's retained leading blank line before a body rewrite.
    #[must_use]
    pub(in crate::pending_work) fn body_region(body: &str) -> &str {
        body.strip_prefix('\n').unwrap_or(body)
    }

    /// Reads a required item within a project.
    pub(in crate::pending_work) fn require_item<S>(
        store: &S,
        project: &ProjectName,
        id: &WorkItemId,
    ) -> Result<PendingWorkRecord, LoadItemError>
    where
        S: AppRecordStore<PendingWorkRecord>,
    {
        <S as AppRecordStore<PendingWorkRecord>>::get(store, project, id)
            .map_err(|error| LoadItemError::Store(Box::new(error)))?
            .ok_or_else(|| LoadItemError::ItemNotFound {
                id: id.as_ref().to_string(),
            })
    }

    /// Inserts a note record, then upserts its open index entry.
    ///
    /// # Panics
    ///
    /// Panics if the store's `insert` violates its contract by returning a record
    /// without a canonical [`WorkItemId`].
    pub(in crate::pending_work) fn create_item<S>(
        store: &S,
        project: &ProjectName,
        new: NewItem,
    ) -> Result<CreatedItem, CreateItemError>
    where
        S: AppRecordStore<PendingWorkRecord>
            + AppRecordStore<IndexEntry>
            + AppRecordStore<IndexSection>,
    {
        let target_section = new.section.clone();
        // Read sections before writing so an invalid index leaves no orphaned note.
        let existing = <S as AppRecordStore<IndexSection>>::list(store, project)
            .map_err(|error| CreateItemError::ReadSections(Box::new(error)))?;
        let created_section = target_section
            .as_deref()
            .filter(|label| SECTION_LABELS.contains(label))
            .filter(|label| {
                !existing
                    .iter()
                    .any(|section| normalize_section_label(&section.label) == *label)
            })
            .map(str::to_string);

        let record = <S as AppRecordStore<PendingWorkRecord>>::insert(store, project, new)
            .map_err(|error| CreateItemError::InsertRecord(Box::new(error)))?;
        let id = record
            .id
            .as_item()
            .expect("inserted record carries a canonical id")
            .clone();
        <S as AppRecordStore<IndexEntry>>::insert(
            store,
            project,
            IndexEntry {
                id,
                state: IndexEntryState::Open,
                // Preserve the raw label; the adapter owns placement.
                section: target_section.unwrap_or_default(),
            },
        )
        .map_err(|error| CreateItemError::InsertIndex {
            project: project.clone(),
            created_section: created_section.clone(),
            source: Box::new(error),
        })?;

        Ok(CreatedItem {
            record,
            created_section,
        })
    }

    #[cfg(test)]
    mod tests {
        use pwf_models::pending_work::{ProjectName, TaskTitle, Timestamp, WorkItemId};

        use super::{LoadItemError, create_item, require_item};
        use crate::{
            pending_work::resolve::testing::staged,
            ports::pending_work_record::{IndexEntryState, NewItem, RecordId},
            testing::InMemoryStore,
        };

        fn pwf() -> ProjectName {
            ProjectName::try_new("pwf").unwrap()
        }

        fn new_item(section: Option<&str>) -> NewItem {
            NewItem {
                prompt: "do the thing".to_string(),
                title: TaskTitle::try_new("ship it").unwrap(),
                created: Timestamp::new("2026-07-15"),
                section: section.map(str::to_string),
                prereq: None,
                effort: None,
                tags: None,
            }
        }

        fn staged_store() -> InMemoryStore {
            InMemoryStore::default().with_prefix("pwf", "PWF")
        }

        #[test]
        fn create_item_inserts_record_and_open_index_entry() {
            let store = staged_store();

            let created = create_item(&store, &pwf(), new_item(None)).unwrap();

            let id = WorkItemId::try_new("PWF-0001").unwrap();
            assert_eq!(created.record.id, RecordId::Item(id.clone()));
            assert_eq!(created.created_section, None);
            let items = store.items("pwf");
            assert_eq!(items.len(), 1, "record must be inserted");
            assert_eq!(items[0].id, RecordId::Item(id.clone()));
            let entries = store.entries("pwf");
            assert_eq!(entries.len(), 1, "open index entry must be upserted");
            assert_eq!(entries[0].id, id);
            assert_eq!(entries[0].state, IndexEntryState::Open);
            assert_eq!(entries[0].section, "");
        }

        #[test]
        fn create_item_reports_created_section_when_region_absent() {
            let store = staged_store();

            let created = create_item(&store, &pwf(), new_item(Some("Human"))).unwrap();

            assert_eq!(created.created_section.as_deref(), Some("Human"));
            assert_eq!(store.entries("pwf")[0].section, "Human");
        }

        #[test]
        fn create_item_does_not_report_existing_empty_section_region() {
            let store = staged_store().with_sections("pwf", &["Human"]);

            let created = create_item(&store, &pwf(), new_item(Some("Human"))).unwrap();

            assert_eq!(created.created_section, None);
        }

        #[test]
        fn create_item_matches_section_aliases_like_the_legacy_read_headers() {
            let store = staged_store().with_sections("pwf", &["Futuro"]);

            let created = create_item(&store, &pwf(), new_item(Some("Future"))).unwrap();

            assert_eq!(created.created_section, None);
        }

        #[test]
        fn require_item_returns_staged_record() {
            let (store, _registry) = staged();
            let id = WorkItemId::try_new("PWF-0001").unwrap();

            let record = require_item(&store, &pwf(), &id).unwrap();

            assert_eq!(record.id, RecordId::Item(id));
        }

        #[test]
        fn require_item_missing_maps_to_legacy_not_found_display() {
            let (store, _registry) = staged();
            let id = WorkItemId::try_new("PWF-9999").unwrap();

            let error = require_item(&store, &pwf(), &id).unwrap_err();

            assert!(matches!(
                error,
                LoadItemError::ItemNotFound { ref id } if id == "PWF-9999"
            ));
            assert_eq!(
                error.to_string(),
                "Open pending-work item not found: PWF-9999"
            );
        }

        #[test]
        fn created_item_carries_record_and_section_fact() {
            let store = staged_store();
            let created = create_item(&store, &pwf(), new_item(Some("Low-prio"))).unwrap();
            assert_eq!(
                created.record.id,
                RecordId::Item(WorkItemId::try_new("PWF-0001").unwrap())
            );
            assert_eq!(created.record.title, "ship it");
            assert_eq!(created.created_section.as_deref(), Some("Low-prio"));
        }
    }
}

pub(in crate::pending_work) mod tag_policy {
    use pwf_models::pending_work::{Tag, Tags};

    pub(in crate::pending_work) fn parse_values(values: &[String]) -> Result<Tags, ParseTagsError> {
        if values.is_empty() {
            return Err(ParseTagsError::MissingTag { raw: String::new() });
        }
        let mut tags = Vec::new();
        for value in values {
            let segments: Vec<&str> = value.split(',').collect();
            if segments.iter().any(|raw| raw.trim().is_empty()) {
                return Err(ParseTagsError::MissingTag { raw: value.clone() });
            }
            for raw in segments {
                let normalized = raw.trim().to_ascii_lowercase().replace('-', "_");
                let tag =
                    Tag::try_from(normalized.as_str()).map_err(|_| ParseTagsError::InvalidTag {
                        raw: raw.to_string(),
                    })?;
                if !tags.contains(&tag) {
                    tags.push(tag);
                }
            }
        }
        Tags::try_new(tags).map_err(|_| ParseTagsError::MissingTag { raw: String::new() })
    }

    pub(in crate::pending_work) fn parse_frontmatter(raw: &str) -> Result<Tags, ParseTagsError> {
        let trimmed = raw.trim();
        let Some(inner) = trimmed
            .strip_prefix('[')
            .and_then(|value| value.strip_suffix(']'))
        else {
            return Err(ParseTagsError::InvalidFrontmatter {
                raw: raw.to_string(),
            });
        };
        if inner.trim().is_empty() {
            return Err(ParseTagsError::MissingTag {
                raw: raw.to_string(),
            });
        }
        parse_values(&[inner.to_string()])
    }

    #[must_use]
    pub(in crate::pending_work) fn merge(existing: &Tags, appended: &Tags) -> Tags {
        let mut merged = existing.iter().cloned().collect::<Vec<_>>();
        for tag in appended.iter() {
            if !merged.contains(tag) {
                merged.push(tag.clone());
            }
        }
        Tags::try_new(merged).expect("the existing tag collection is non-empty")
    }

    #[must_use]
    pub(in crate::pending_work) fn contains_all(stored: &Tags, requested: &Tags) -> bool {
        requested
            .iter()
            .all(|requested| stored.iter().any(|stored| stored == requested))
    }

    #[must_use]
    #[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
    pub(in crate::pending_work) enum ParseTagsError {
        #[error("missing tag value: {raw:?}")]
        MissingTag { raw: String },
        #[error("invalid tag value: {raw:?}")]
        InvalidTag { raw: String },
        #[error("invalid tags frontmatter: {raw:?}")]
        InvalidFrontmatter { raw: String },
    }

    impl ParseTagsError {
        pub(in crate::pending_work) fn raw(&self) -> &str {
            match self {
                Self::MissingTag { raw }
                | Self::InvalidTag { raw }
                | Self::InvalidFrontmatter { raw } => raw,
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use pwf_models::pending_work::Tags;

        use super::{ParseTagsError, contains_all, merge, parse_frontmatter, parse_values};

        fn values(tags: &Tags) -> Vec<&str> {
            tags.iter().map(AsRef::as_ref).collect()
        }

        #[test]
        fn cli_values_normalize_and_deduplicate_in_encounter_order() {
            let tags = parse_values(&[
                "SQLite,csharp-export".to_string(),
                "sqlite".to_string(),
                " godot ".to_string(),
                "csharp_export".to_string(),
            ])
            .unwrap();

            assert_eq!(values(&tags), ["sqlite", "csharp_export", "godot"]);
        }

        #[test]
        fn cli_values_reject_empty_segments_and_invalid_characters() {
            for raw in [
                "",
                ",",
                "sqlite,",
                "_sqlite",
                "sqlite_",
                "sqlite__export",
                "c#",
            ] {
                let error = parse_values(&[raw.to_string()]).unwrap_err();
                assert_eq!(error.raw(), raw);
            }
        }

        #[test]
        fn frontmatter_requires_an_inline_array_and_reuses_tag_validation() {
            let tags = parse_frontmatter("[SQLite, csharp-export]").unwrap();
            assert_eq!(values(&tags), ["sqlite", "csharp_export"]);

            assert!(matches!(
                parse_frontmatter("sqlite"),
                Err(ParseTagsError::InvalidFrontmatter { .. })
            ));
            assert!(parse_frontmatter("[]").is_err());
        }

        #[test]
        fn merge_and_inclusion_preserve_set_semantics() {
            let existing = parse_frontmatter("[sqlite, godot]").unwrap();
            let appended = parse_values(&["godot,csharp-export".to_string()]).unwrap();
            let merged = merge(&existing, &appended);

            assert_eq!(values(&merged), ["sqlite", "godot", "csharp_export"]);
            assert!(contains_all(&merged, &appended));
            assert!(!contains_all(&appended, &existing));
        }
    }
}

pub(in crate::pending_work) mod title {
    use prompt_lanes::parse;
    use pwf_models::pending_work::{TaskTitle, TaskTitleError};

    pub(in crate::pending_work) fn inferred(prompt: &str) -> Result<TaskTitle, TaskTitleError> {
        TaskTitle::try_new(parse(prompt).title)
    }
}

pub(in crate::pending_work) mod pending_work_update {
    use pwf_models::pending_work::{Tags, WorkItemId, WorkItemStatus};

    use crate::{
        pending_work::{
            dto::{PendingWorkItemIdentity, PreparedPendingWorkUpdate},
            logic::{
                commit_provenance, identifier,
                note_body::{append_lanes, append_report_block, render},
                prerequisite::{PrerequisiteValidationError, validate_and_merge},
                store_util::body_region,
                tag_policy,
            },
            update_pending_work_item::{
                UpdatePendingWorkError, UpdatePendingWorkItem, UpdatePendingWorkItemOk,
            },
        },
        ports::{
            app_record::AppRecordStore,
            pending_work_record::{ItemPatch, PendingWorkRecord},
        },
        project::ProjectRegistry,
    };
    pub(in crate::pending_work) fn prepare(
        command: &UpdatePendingWorkItem,
        store: &impl AppRecordStore<PendingWorkRecord>,
        projects: &ProjectRegistry,
    ) -> Result<PreparedPendingWorkUpdate, UpdatePendingWorkError> {
        let commits = commit_provenance::normalize(&command.commits);
        let tags = parse_tags(&command.tags)?;
        let edits_body = command.prompt.is_some()
            || command.title.is_some()
            || !command.prereq.is_empty()
            || command.clear_prereq
            || command.append.is_some()
            || command.effort.is_some()
            || tags.is_some()
            || command.tags_clear;
        if !edits_body && commits.is_none() && command.append_report.is_none() {
            return Err(UpdatePendingWorkError::NothingToUpdate);
        }

        let not_found = || UpdatePendingWorkError::ItemNotFound {
            id: command.id.clone(),
        };
        let identifier = identifier::parse(&command.id).ok_or_else(not_found)?;
        let project = projects
            .get_project_name_by(&identifier)
            .ok_or_else(not_found)?;
        let record = store
            .get(project, &identifier)
            .map_err(|error| UpdatePendingWorkError::WriteStore(Box::new(error)))?
            .ok_or_else(not_found)?;
        let identity = PendingWorkItemIdentity {
            project: project.clone(),
            identifier,
        };

        if record.status == WorkItemStatus::Active {
            prepare_open_item(
                store,
                projects,
                identity,
                command,
                commits.as_deref(),
                tags.as_ref(),
                &record,
            )
        } else if edits_body {
            Err(UpdatePendingWorkError::ClosedItemAmendOnly {
                id: command.id.clone(),
            })
        } else {
            amend_closed_item(identity, command, commits.as_deref(), &record)
        }
    }

    pub(in crate::pending_work) fn persist(
        prepared: PreparedPendingWorkUpdate,
        store: &impl AppRecordStore<PendingWorkRecord>,
    ) -> Result<UpdatePendingWorkItemOk, UpdatePendingWorkError> {
        store
            .update(
                &prepared.identity.project,
                &prepared.identity.identifier,
                prepared.patch,
            )
            .map_err(|error| UpdatePendingWorkError::WriteStore(Box::new(error)))?;
        Ok(prepared.outcome)
    }

    fn prepare_open_item<S>(
        store: &S,
        projects: &ProjectRegistry,
        identity: PendingWorkItemIdentity,
        command: &UpdatePendingWorkItem,
        commits: Option<&str>,
        tags: Option<&Tags>,
        record: &PendingWorkRecord,
    ) -> Result<PreparedPendingWorkUpdate, UpdatePendingWorkError>
    where
        S: AppRecordStore<PendingWorkRecord>,
    {
        let new_title = command
            .title
            .as_ref()
            .map_or_else(|| record.title.clone(), ToString::to_string);

        let mut patch = ItemPatch {
            body: compute_body(command, record)?,
            ..ItemPatch::default()
        };
        if command.title.is_some() {
            patch.title.clone_from(&command.title);
        }
        if command.clear_prereq {
            patch.prereq = Some(None);
        } else if !command.prereq.is_empty() {
            let merged =
                validate_and_merge(record.prereq.as_deref(), &command.prereq, store, projects)
                    .map_err(map_prerequisite_error)?;
            patch.prereq = Some(Some(merged));
        }
        if let Some(commits) = commits {
            patch.commits = Some(Some(commits.to_string()));
        }
        if let Some(effort) = command.effort {
            patch.effort = Some(effort);
        }
        patch.tags = resolve_tags(tags, command.tags_clear, &identity.identifier, record)?;

        let outcome = UpdatePendingWorkItemOk::OpenItemEdit {
            id: identity.identifier.as_ref().to_string(),
            project: identity.project.as_ref().to_string(),
            title: new_title,
        };
        Ok(PreparedPendingWorkUpdate {
            identity,
            patch,
            outcome,
        })
    }

    /// Computes a replacement body when a prompt, lane, or report edit requires one.
    ///
    /// Prompt replacement starts from a new body; lane and report edits build on that result.
    fn compute_body(
        command: &UpdatePendingWorkItem,
        record: &PendingWorkRecord,
    ) -> Result<Option<String>, UpdatePendingWorkError> {
        let base = body_region(&record.body);
        let mut body: Option<String> = command.prompt.as_deref().map(render);
        if let Some(append) = &command.append {
            let current = body.as_deref().unwrap_or(base);
            body = Some(append_lanes(current, append).ok_or(UpdatePendingWorkError::EmptyAppend)?);
        }
        if let Some(report) = &command.append_report {
            let current = body.as_deref().unwrap_or(base);
            body = Some(
                append_report_block(current, report).ok_or(UpdatePendingWorkError::EmptyReport)?,
            );
        }
        Ok(body)
    }

    /// Resolves the tri-state `ItemPatch.tags` value: unchanged, cleared, or replaced.
    #[expect(
        clippy::option_option,
        reason = "preserves ItemPatch.tags tri-state semantics"
    )]
    fn resolve_tags(
        appended: Option<&Tags>,
        clear: bool,
        id: &WorkItemId,
        record: &PendingWorkRecord,
    ) -> Result<Option<Option<Tags>>, UpdatePendingWorkError> {
        let Some(appended) = appended else {
            return Ok(clear.then_some(None));
        };
        let tags = if clear {
            appended.clone()
        } else if let Some(existing) = record.tags.as_deref() {
            let existing = tag_policy::parse_frontmatter(existing).map_err(|error| {
                UpdatePendingWorkError::InvalidTagsFrontmatter {
                    id: id.as_ref().to_string(),
                    raw: error.raw().to_string(),
                }
            })?;
            tag_policy::merge(&existing, appended)
        } else {
            appended.clone()
        };
        Ok(Some(Some(tags)))
    }

    fn amend_closed_item(
        identity: PendingWorkItemIdentity,
        command: &UpdatePendingWorkItem,
        commits: Option<&str>,
        record: &PendingWorkRecord,
    ) -> Result<PreparedPendingWorkUpdate, UpdatePendingWorkError> {
        let mut patch = ItemPatch::default();
        let mut changes = Vec::new();
        if let Some(commits) = commits {
            patch.commits = Some(Some(commits.to_string()));
            changes.push(commits_change(commits));
        }
        if let Some(report) = &command.append_report {
            let base = body_region(&record.body);
            patch.body =
                Some(append_report_block(base, report).ok_or(UpdatePendingWorkError::EmptyReport)?);
            changes.push("report appended".to_string());
        }
        let outcome = UpdatePendingWorkItemOk::Changed {
            id: identity.identifier.as_ref().to_string(),
            changes,
        };
        Ok(PreparedPendingWorkUpdate {
            identity,
            patch,
            outcome,
        })
    }

    fn commits_change(commits: &str) -> String {
        format!("commits: {commits}")
    }

    fn map_prerequisite_error(error: PrerequisiteValidationError) -> UpdatePendingWorkError {
        match error {
            PrerequisiteValidationError::InvalidId { raw } => {
                UpdatePendingWorkError::InvalidPrereqId { raw }
            }
            PrerequisiteValidationError::MissingId => UpdatePendingWorkError::MissingPrereqId,
            PrerequisiteValidationError::UnknownIds { ids } => {
                UpdatePendingWorkError::UnknownPrereqIds { ids }
            }
        }
    }

    fn parse_tags(values: &[String]) -> Result<Option<Tags>, UpdatePendingWorkError> {
        if values.is_empty() {
            return Ok(None);
        }
        tag_policy::parse_values(values).map(Some).map_err(|error| {
            UpdatePendingWorkError::InvalidTag {
                raw: error.raw().to_string(),
            }
        })
    }
}

pub(in crate::pending_work) mod pending_work_creation {
    use std::path::PathBuf;

    use pwf_models::pending_work::ProjectName;

    use crate::{
        pending_work::{
            add_pending_work_item::{AddPendingWorkError, AddPendingWorkItemOk},
            logic::store_util,
        },
        project::ProjectRegistry,
    };

    pub(in crate::pending_work) fn project_mapped(
        project_identifier: &str,
        projects: &ProjectRegistry,
    ) -> Result<ProjectName, AddPendingWorkError> {
        let project = projects.resolve(project_identifier)?.clone();
        let not_mapped = || AddPendingWorkError::ProjectHasNoDirectorySource {
            project: project.to_string(),
        };
        if projects
            .get_repository_by(&project)
            .is_none_or(|repo| repo.trim().is_empty())
        {
            return Err(not_mapped());
        }
        Ok(project)
    }

    pub(in crate::pending_work) fn added_item(
        project: &ProjectName,
        created: store_util::CreatedItem,
    ) -> AddPendingWorkItemOk {
        let id = created
            .record
            .id
            .as_item()
            .expect("inserted record carries a canonical id")
            .as_ref()
            .to_string();
        AddPendingWorkItemOk {
            id,
            project: project.as_ref().to_string(),
            title: created.record.title,
            note_path: PathBuf::from(created.record.locator),
            created_section: created.created_section,
        }
    }
}

pub(in crate::pending_work) mod pending_work_closing {
    use pwf_models::pending_work::{ProjectName, Timestamp, WorkItemId, WorkItemStatus};

    use crate::{
        pending_work::{
            add_pending_work_item::{
                AddPendingWorkError, AddPendingWorkItemOk, PendingWorkSection,
            },
            complete_pending_work::{
                ClosedItemAction, CompletePendingWorkError, CompletePendingWorkOk,
            },
            logic::{
                commit_provenance, identifier,
                note_body::append_report,
                pending_work_creation::{added_item, project_mapped},
                store_util::{self, LoadItemError, body_region},
                title,
            },
        },
        ports::{
            app_record::AppRecordStore,
            pending_work_record::{
                IndexEntry, IndexEntryState, IndexSection, ItemPatch, Materialization, NewItem,
                PendingWorkRecord,
            },
        },
        project::ProjectRegistry,
    };

    mod queue {
        use pwf_models::pending_work::{Timestamp, WorkItemId};

        use crate::{
            pending_work::section,
            ports::pending_work_record::{IndexEntry, IndexEntryState, IndexSection},
        };

        const SECTION_CAPS: &[(&str, usize)] =
            &[("General", 6), ("Low-prio", 3), ("Future", 3), ("Human", 3)];

        pub(super) struct CloseDecisions {
            pub(super) evicted_ids: Vec<WorkItemId>,
            pub(super) normalize_futuro_header: bool,
            pub(super) mark_target: bool,
        }

        pub(super) fn close_decisions(
            entries: &[IndexEntry],
            sections: &[IndexSection],
            id: &WorkItemId,
            completed: &Timestamp,
        ) -> CloseDecisions {
            let normalize_futuro_header = sections
                .iter()
                .any(|section| is_futuro_label(&section.label));

            let Some(target) = entries
                .iter()
                .find(|entry| &entry.id == id && entry.state == IndexEntryState::Open)
            else {
                return CloseDecisions {
                    evicted_ids: Vec::new(),
                    normalize_futuro_header,
                    mark_target: false,
                };
            };

            CloseDecisions {
                evicted_ids: evict_beyond_cap(entries, &target.section, id, completed),
                normalize_futuro_header,
                mark_target: true,
            }
        }

        fn evict_beyond_cap(
            entries: &[IndexEntry],
            target_section: &str,
            id: &WorkItemId,
            completed: &Timestamp,
        ) -> Vec<WorkItemId> {
            let target_section = canonical_section(target_section);
            let Some(cap) = section_cap(&target_section) else {
                return Vec::new();
            };

            let mut done: Vec<(&str, &WorkItemId)> = entries
                .iter()
                .filter_map(|entry| match &entry.state {
                    IndexEntryState::Done(entry_completed)
                        if canonical_section(&entry.section) == target_section =>
                    {
                        Some((entry_completed.as_str(), &entry.id))
                    }
                    IndexEntryState::Open | IndexEntryState::Done(_) => None,
                })
                .collect();
            done.push((completed.as_str(), id));

            if done.len() <= cap {
                return Vec::new();
            }

            let evict_count = done.len() - cap;
            done.sort_by(|left, right| left.0.cmp(right.0).then(left.1.cmp(right.1)));
            done.into_iter()
                .take(evict_count)
                .map(|(_, evicted_id)| evicted_id.clone())
                .collect()
        }

        pub(super) fn is_futuro_label(label: &str) -> bool {
            label.trim().eq_ignore_ascii_case("futuro")
        }

        fn section_cap(section_canonical: &str) -> Option<usize> {
            SECTION_CAPS
                .iter()
                .find(|(name, _)| *name == section_canonical)
                .map(|(_, cap)| *cap)
        }

        fn canonical_section(label: &str) -> String {
            if label.trim().is_empty() || label.trim() == "General" {
                return "General".to_string();
            }
            section::alias(label).map_or_else(|| label.trim().to_lowercase(), str::to_string)
        }

        #[cfg(test)]
        mod tests {
            use super::*;

            fn id(raw: &str) -> WorkItemId {
                WorkItemId::try_new(raw).unwrap()
            }

            fn done(raw_id: &str, date: &str, section: &str) -> IndexEntry {
                IndexEntry {
                    id: id(raw_id),
                    state: IndexEntryState::Done(Timestamp::new(date)),
                    section: section.to_string(),
                }
            }

            fn open(raw_id: &str, section: &str) -> IndexEntry {
                IndexEntry {
                    id: id(raw_id),
                    state: IndexEntryState::Open,
                    section: section.to_string(),
                }
            }

            #[test]
            fn cap_boundary_evicts_single_oldest_beyond_cap() {
                let mut entries: Vec<IndexEntry> = (1..=6)
                    .map(|number| {
                        done(
                            &format!("PWF-{number:04}"),
                            &format!("2026-01-{number:02}"),
                            "",
                        )
                    })
                    .collect();
                entries.push(open("PWF-0007", ""));

                let decisions = close_decisions(
                    &entries,
                    &[],
                    &id("PWF-0007"),
                    &Timestamp::new("2026-07-07"),
                );

                assert_eq!(decisions.evicted_ids, vec![id("PWF-0001")]);
                assert!(decisions.mark_target);
            }

            #[test]
            fn tied_completed_dates_break_by_ascending_id() {
                let entries = vec![
                    done("PWF-0002", "2026-01-01", "Human"),
                    done("PWF-0001", "2026-01-01", "Human"),
                    done("PWF-0003", "2026-01-02", "Human"),
                    open("PWF-0004", "Human"),
                ];

                let decisions = close_decisions(
                    &entries,
                    &[],
                    &id("PWF-0004"),
                    &Timestamp::new("2026-01-03"),
                );

                assert_eq!(decisions.evicted_ids, vec![id("PWF-0001")]);
            }

            #[test]
            fn missing_completed_date_sorts_before_any_dated_entry() {
                let entries = vec![
                    done("PWF-0001", "", "Human"),
                    done("PWF-0002", "2026-01-01", "Human"),
                    done("PWF-0003", "2026-01-02", "Human"),
                    open("PWF-0004", "Human"),
                ];

                let decisions = close_decisions(
                    &entries,
                    &[],
                    &id("PWF-0004"),
                    &Timestamp::new("2026-01-03"),
                );

                assert_eq!(decisions.evicted_ids, vec![id("PWF-0001")]);
            }

            #[test]
            fn section_without_a_cap_evicts_nothing() {
                let mut entries: Vec<IndexEntry> = (1..=9)
                    .map(|number| {
                        done(
                            &format!("PWF-{number:04}"),
                            &format!("2026-01-{number:02}"),
                            "Someday",
                        )
                    })
                    .collect();
                entries.push(open("PWF-0010", "Someday"));

                let decisions = close_decisions(
                    &entries,
                    &[],
                    &id("PWF-0010"),
                    &Timestamp::new("2026-07-07"),
                );

                assert!(decisions.evicted_ids.is_empty());
            }

            #[test]
            fn raw_section_label_aliases_before_cap_lookup() {
                let mut entries: Vec<IndexEntry> = (1..=3)
                    .map(|number| {
                        done(
                            &format!("PWF-{number:04}"),
                            &format!("2026-01-{number:02}"),
                            "futuro",
                        )
                    })
                    .collect();
                entries.push(open("PWF-0004", "Futuro"));

                let decisions = close_decisions(
                    &entries,
                    &[],
                    &id("PWF-0004"),
                    &Timestamp::new("2026-07-07"),
                );

                assert_eq!(decisions.evicted_ids, vec![id("PWF-0001")]);
            }

            #[test]
            fn futuro_header_normalizes_when_target_entry_is_missing() {
                let sections = [IndexSection {
                    label: "Futuro".to_string(),
                }];

                let decisions = close_decisions(
                    &[],
                    &sections,
                    &id("PWF-0001"),
                    &Timestamp::new("2026-07-07"),
                );

                assert!(decisions.normalize_futuro_header);
                assert!(!decisions.mark_target);
                assert!(decisions.evicted_ids.is_empty());
            }

            #[test]
            fn futuro_header_check_is_case_insensitive_and_trims_whitespace() {
                let sections = [IndexSection {
                    label: "  FUTURO  ".to_string(),
                }];

                let decisions = close_decisions(
                    &[],
                    &sections,
                    &id("PWF-0001"),
                    &Timestamp::new("2026-07-07"),
                );

                assert!(decisions.normalize_futuro_header);
            }

            #[test]
            fn unrelated_headers_do_not_normalize() {
                let sections = [
                    IndexSection {
                        label: "Human".to_string(),
                    },
                    IndexSection {
                        label: "Future".to_string(),
                    },
                ];

                let decisions = close_decisions(
                    &[],
                    &sections,
                    &id("PWF-0001"),
                    &Timestamp::new("2026-07-07"),
                );

                assert!(!decisions.normalize_futuro_header);
            }
        }
    }

    use queue::{close_decisions, is_futuro_label};
    pub(in crate::pending_work) enum CloseError {
        ItemNotFound {
            id: String,
        },
        UnknownPrefix {
            pending_work_identifier: String,
            prefix: String,
        },
        EmptyReport,
        WriteStore(Box<dyn std::error::Error + Send + Sync>),
        ReviewTask(AddPendingWorkError),
    }

    impl CloseError {
        pub(in crate::pending_work) fn into_complete(self) -> CompletePendingWorkError {
            match self {
                Self::ItemNotFound { id } => CompletePendingWorkError::ItemNotFound { id },
                Self::UnknownPrefix {
                    pending_work_identifier,
                    prefix,
                } => CompletePendingWorkError::UnknownPrefix {
                    pending_work_identifier,
                    prefix,
                },
                Self::EmptyReport => CompletePendingWorkError::EmptyReport,
                Self::WriteStore(source) => CompletePendingWorkError::WriteStore(source),
                Self::ReviewTask(source) => CompletePendingWorkError::ReviewTask(source),
            }
        }
    }

    /// Closes an item through the flow shared by done and cancel.
    ///
    /// One patch applies report and status fields. Only note-backed records rotate the queue,
    /// because inline and missing-note records have no file-backed queue entry.
    #[expect(
        clippy::too_many_arguments,
        reason = "done and cancel share this orchestration"
    )]
    pub(in crate::pending_work) fn perform_close<S>(
        store: &S,
        projects: &ProjectRegistry,
        action: ClosedItemAction,
        id: &str,
        completed: &str,
        report: Option<&str>,
        commits: &[String],
        review: bool,
    ) -> Result<CompletePendingWorkOk, CloseError>
    where
        S: AppRecordStore<PendingWorkRecord>
            + AppRecordStore<IndexEntry>
            + AppRecordStore<IndexSection>,
    {
        let commits_value = commit_provenance::normalize(commits);
        let pending_work_identifier =
            identifier::parse(id).ok_or_else(|| CloseError::ItemNotFound { id: id.to_string() })?;
        let project_id = pending_work_identifier
            .as_ref()
            .split_once('-')
            .map_or("", |(project_id, _)| project_id);
        let project = projects
            .get_project_name_by(&pending_work_identifier)
            .ok_or_else(|| CloseError::UnknownPrefix {
                pending_work_identifier: pending_work_identifier.to_string(),
                prefix: project_id.to_string(),
            })?;
        let record =
            store_util::require_item(store, project, &pending_work_identifier).map_err(map_load)?;
        if record.status != WorkItemStatus::Active {
            return Err(CloseError::ItemNotFound {
                id: pending_work_identifier.as_ref().to_string(),
            });
        }
        let title = record.title.clone();
        let mut patch = ItemPatch {
            status: Some(action.status()),
            completed: Some(Some(Timestamp::new(completed))),
            ..ItemPatch::default()
        };
        if let Some(report) = report {
            let body =
                append_report(body_region(&record.body), report).ok_or(CloseError::EmptyReport)?;
            patch.body = Some(body);
        }
        if let Some(commits) = &commits_value {
            patch.commits = Some(Some(commits.clone()));
        }
        <S as AppRecordStore<PendingWorkRecord>>::update(
            store,
            project,
            &pending_work_identifier,
            patch,
        )
        .map_err(|error| CloseError::WriteStore(Box::new(error)))?;

        let (evicted_ids, futuro_renamed) =
            if matches!(record.materialization, Materialization::NoteFile) {
                rotate_done_queue(store, project, &pending_work_identifier, completed)?
            } else {
                (Vec::new(), false)
            };

        let review_item = review
            .then(|| {
                spawn_review(
                    store,
                    projects,
                    project,
                    &pending_work_identifier,
                    completed,
                    commits_value.as_deref(),
                )
            })
            .transpose()?;

        Ok(CompletePendingWorkOk {
            id: pending_work_identifier,
            project: project.clone(),
            title,
            action,
            evicted_ids,
            futuro_renamed_project: futuro_renamed.then(|| project.clone()),
            review_item,
        })
    }

    fn map_load(error: LoadItemError) -> CloseError {
        match error {
            LoadItemError::ItemNotFound { id } => CloseError::ItemNotFound { id },
            LoadItemError::Store(source) => CloseError::WriteStore(source),
        }
    }

    /// Applies header normalization, the closed entry, and cap-based evictions to the index.
    fn rotate_done_queue<S>(
        store: &S,
        project: &ProjectName,
        id: &WorkItemId,
        completed: &str,
    ) -> Result<(Vec<WorkItemId>, bool), CloseError>
    where
        S: AppRecordStore<IndexEntry> + AppRecordStore<IndexSection>,
    {
        let entries = <S as AppRecordStore<IndexEntry>>::list(store, project)
            .map_err(|error| CloseError::WriteStore(Box::new(error)))?;
        let sections = <S as AppRecordStore<IndexSection>>::list(store, project)
            .map_err(|error| CloseError::WriteStore(Box::new(error)))?;
        let completed = Timestamp::new(completed);
        let decisions = close_decisions(&entries, &sections, id, &completed);

        if decisions.normalize_futuro_header {
            rename_futuro_headers(store, project, &sections)?;
        }
        if decisions.mark_target {
            <S as AppRecordStore<IndexEntry>>::update(
                store,
                project,
                id,
                IndexEntry {
                    id: id.clone(),
                    state: IndexEntryState::Done(completed),
                    section: String::new(),
                },
            )
            .map_err(|error| CloseError::WriteStore(Box::new(error)))?;
        }
        for evicted in &decisions.evicted_ids {
            <S as AppRecordStore<IndexEntry>>::delete(store, project, evicted)
                .map_err(|error| CloseError::WriteStore(Box::new(error)))?;
        }
        Ok((decisions.evicted_ids, decisions.normalize_futuro_header))
    }

    /// Renames every `## Futuro` header to `## Future` through the section port.
    fn rename_futuro_headers<S>(
        store: &S,
        project: &ProjectName,
        sections: &[IndexSection],
    ) -> Result<(), CloseError>
    where
        S: AppRecordStore<IndexSection>,
    {
        for section in sections.iter().filter(|s| is_futuro_label(&s.label)) {
            <S as AppRecordStore<IndexSection>>::update(
                store,
                project,
                &section.label,
                IndexSection {
                    label: "Future".to_string(),
                },
            )
            .map_err(|error| CloseError::WriteStore(Box::new(error)))?;
        }
        Ok(())
    }

    fn spawn_review<S>(
        store: &S,
        projects: &ProjectRegistry,
        project: &ProjectName,
        reviewed: &WorkItemId,
        completed: &str,
        commits: Option<&str>,
    ) -> Result<AddPendingWorkItemOk, CloseError>
    where
        S: AppRecordStore<PendingWorkRecord>
            + AppRecordStore<IndexEntry>
            + AppRecordStore<IndexSection>,
    {
        project_mapped(project.as_ref(), projects).map_err(CloseError::ReviewTask)?;
        let prompt = review_task_prompt(reviewed.as_ref(), commits);
        let review_title = title::inferred(&prompt)
            .map_err(AddPendingWorkError::from)
            .map_err(CloseError::ReviewTask)?;
        let created = store_util::create_item(
            store,
            project,
            NewItem {
                title: review_title,
                prompt,
                created: Timestamp::new(completed),
                section: Some(PendingWorkSection::Human.as_str().to_string()),
                prereq: None,
                effort: None,
                tags: None,
            },
        )
        .map_err(|source| {
            CloseError::ReviewTask(AddPendingWorkError::WriteStore {
                diagnostics:
                    crate::pending_work::add_pending_work_item::AddPendingWorkDiagnostics {
                        project: project.to_string(),
                        created_section: source
                            .created_section()
                            .map(|(_, section)| section.to_string()),
                    },
                source,
            })
        })?;
        Ok(added_item(project, created))
    }

    pub(in crate::pending_work) fn review_task_prompt(
        reviewed_id: &str,
        range: Option<&str>,
    ) -> String {
        let (title, diff) = match range {
            Some(range) => (
                format!("review {reviewed_id}, commits: {range}"),
                format!("git-tools diff {range}"),
            ),
            None => (
                format!("review {reviewed_id}"),
                "git-tools diff".to_string(),
            ),
        };
        format!("{title} / {diff} / git-tools diff-subrepos")
    }
}
