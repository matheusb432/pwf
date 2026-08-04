use super::{add_task, show_task};
pub(in crate::task) mod finding {
    use pwf_models::{project::Project, task::TaskId};

    use crate::{
        ports::task_record::{TaskRecord, TaskStore},
        task::{dto::TaskView, find_active_task::FindActiveTaskError, logic::enrich},
    };

    pub(in crate::task) fn find_active_task_in_projects(
        store: &impl TaskStore,
        projects: &[Project],
        task_id: &TaskId,
    ) -> Result<(TaskRecord, TaskView), FindActiveTaskError> {
        let project_id = task_id.project_id();
        let project = projects
            .iter()
            .find(|project| project.id == project_id)
            .ok_or_else(|| FindActiveTaskError::UnknownProjectId {
                task_id: task_id.clone(),
                project_id,
            })?;
        let records = store
            .list(project)
            .map_err(|error| FindActiveTaskError::ReadStore(Box::new(error)))?;
        let mut matched = records
            .into_iter()
            .filter(|record| enrich::is_active_task(record) && record.id == *task_id);
        let Some(record) = matched.next() else {
            return Err(FindActiveTaskError::TaskNotFound {
                id: task_id.clone(),
            });
        };
        if matched.next().is_some() {
            return Err(FindActiveTaskError::AmbiguousId {
                id: task_id.clone(),
            });
        }
        let task = enrich::enrich(&record, project.source.value())
            .into_task_view(project.title.to_string());
        Ok((record, task))
    }
}

pub(in crate::task) mod commit_provenance {
    pub(in crate::task) fn normalize(values: &[String]) -> Option<String> {
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

pub(in crate::task) mod enrich {
    //! Derives task launchability diagnostics for list and session operations.

    use pwf_models::{
        project::ProjectSourceValue,
        task::{TaskId, TaskStatus},
    };

    use super::{note_body::is_placeholder_prompt, prerequisite, section};
    use crate::{
        ports::task_record::{Materialization, TaskRecord},
        task::dto::TaskView,
    };

    pub(in crate::task) const ISSUE_PLACEHOLDER_PROMPT: &str =
        "Prompt is a placeholder; define a real prompt before launching.";

    /// Canonicalizes known section aliases and trims unknown labels for display.
    #[must_use]
    pub(in crate::task) fn normalize_section_label(label: &str) -> String {
        section::alias(label).map_or_else(|| label.trim().to_string(), str::to_string)
    }

    #[must_use]
    pub(in crate::task) fn normalize_section(section: Option<&str>) -> Option<String> {
        section.map(normalize_section_label)
    }

    /// Returns whether an task is active.
    #[must_use]
    pub(in crate::task) fn is_active_task(task: &TaskRecord) -> bool {
        task.status == TaskStatus::Active
    }

    /// Contains launchability flags and diagnostics derived from a task.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(in crate::task) struct DerivedFlags {
        pub(in crate::task) issues: Vec<String>,
        pub(in crate::task) launchable: bool,
        pub(in crate::task) needs_prompt: bool,
    }

    /// Derives missing-note and placeholder diagnostics.
    #[must_use]
    pub(in crate::task) fn derive_flags(prompt: &str, missing_note: Option<&str>) -> DerivedFlags {
        let mut issues = Vec::new();
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

    /// Contains a task's persisted data and derived diagnostics before project
    /// attachment.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(in crate::task) struct EnrichedTask {
        pub(in crate::task) id: TaskId,
        pub(in crate::task) status: TaskStatus,
        pub(in crate::task) session: String,
        pub(in crate::task) prompt: String,
        pub(in crate::task) project_path: ProjectSourceValue,
        pub(in crate::task) note: String,
        pub(in crate::task) task_file: Option<String>,
        pub(in crate::task) line: usize,
        pub(in crate::task) format: String,
        pub(in crate::task) launchable: bool,
        pub(in crate::task) needs_prompt: bool,
        pub(in crate::task) issues: Vec<String>,
        pub(in crate::task) section: Option<String>,
        pub(in crate::task) prerequisites: Option<pwf_models::task::Prerequisites>,
        pub(in crate::task) effort: Option<String>,
        pub(in crate::task) tags: Option<String>,
        pub(in crate::task) created: Option<String>,
    }

    impl EnrichedTask {
        /// Attaches the managed project name.
        #[must_use]
        pub(in crate::task) fn into_task_view(self, project: String) -> TaskView {
            TaskView {
                id: self.id,
                project,
                status: self.status,
                session: self.session,
                prompt: self.prompt,
                project_path: self.project_path,
                note: self.note,
                task_file: self.task_file,
                line: self.line,
                format: self.format,
                launchable: self.launchable,
                needs_prompt: self.needs_prompt,
                issues: self.issues,
                section: self.section,
                prerequisites: self.prerequisites,
                prerequisite_statuses: Vec::new(),
                effort: self.effort,
                tags: self.tags,
                created: self.created,
            }
        }
    }

    fn missing_note_issue(path: &str) -> String {
        format!("Task note missing: {path}")
    }

    /// Projects a persisted task into the fields consumed by list and session.
    ///
    /// Materialization controls `format` and `task_file`; missing notes add an issue; empty titles
    /// fall back to the task ID; section labels are normalized for display.
    #[must_use]
    pub(in crate::task) fn enrich(
        task: &TaskRecord,
        project_path: &ProjectSourceValue,
    ) -> EnrichedTask {
        let prompt = task.body.trim().to_string();
        let (format, task_file, missing_note) = match &task.materialization {
            Materialization::NoteFile => ("file", Some(task.locator.clone()), None),
            Materialization::MissingNote { expected } => {
                ("file", Some(task.locator.clone()), Some(expected.as_str()))
            }
        };
        let flags = derive_flags(&prompt, missing_note);
        let session = if task.title.trim().is_empty() {
            task.id.to_string()
        } else {
            task.title.clone()
        };
        let (note, line) = task.placement.as_ref().map_or_else(
            || (task.locator.clone(), 1),
            |placement| (placement.index_path.clone(), placement.line),
        );
        EnrichedTask {
            id: task.id.clone(),
            status: task.status,
            session,
            prompt,
            project_path: project_path.clone(),
            note,
            line,
            task_file,
            format: format.to_string(),
            launchable: flags.launchable,
            needs_prompt: flags.needs_prompt,
            issues: flags.issues,
            section: normalize_section(task.section.as_deref()),
            prerequisites: task.prereq.as_deref().and_then(prerequisite::extract),
            effort: task.effort.clone(),
            tags: task.tags.clone(),
            created: task.created.as_ref().map(|ts| ts.as_str().to_string()),
        }
    }

    #[cfg(test)]
    mod tests {
        use pwf_models::{
            project::ProjectSourceValue,
            task::{TaskId, TaskStatus, Timestamp},
        };

        use super::*;
        use crate::ports::task_record::IndexPlacement;

        fn record(body: &str) -> TaskRecord {
            TaskRecord {
                id: TaskId::try_new("PWF-0001").unwrap(),
                title: "tray gui".to_string(),
                status: TaskStatus::Active,
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

        fn project_path() -> ProjectSourceValue {
            ProjectSourceValue::try_new("/project").unwrap()
        }

        #[test]
        fn launchable_when_project_path_is_present_and_prompt_is_real() {
            let enriched = enrich(&record("add startup toggle"), &project_path());
            assert!(enriched.launchable);
            assert!(!enriched.needs_prompt);
            assert!(enriched.issues.is_empty());
            assert_eq!(enriched.prompt, "add startup toggle");
            assert_eq!(enriched.project_path.as_ref(), "/project");
        }

        #[test]
        fn active_item_requires_only_active_status() {
            let active = record("body");
            assert!(is_active_task(&active));

            let unlinked = TaskRecord {
                placement: None,
                ..active.clone()
            };
            assert!(is_active_task(&unlinked));

            let done = TaskRecord {
                status: TaskStatus::Done,
                ..active
            };
            assert!(!is_active_task(&done));
        }

        #[test]
        fn note_and_line_render_the_index_placement_not_the_note_file() {
            let enriched = enrich(&record("body"), &project_path());
            assert_eq!(enriched.note, "/notes/pwf/pwf.md");
            assert_eq!(enriched.line, 7);
            assert_eq!(
                enriched.task_file.as_deref(),
                Some("/notes/pwf/PWF-0001.md")
            );
            assert_eq!(enriched.format, "file");
        }

        #[test]
        fn placeholder_prompt_is_not_launchable() {
            let enriched = enrich(&record("TODO"), &project_path());
            assert!(!enriched.launchable);
            assert!(enriched.needs_prompt);
            assert_eq!(enriched.issues, [ISSUE_PLACEHOLDER_PROMPT.to_string()]);
        }

        #[test]
        fn missing_note_wikilink_needs_attention_with_missing_note_issue() {
            let mut rec = record("");
            rec.materialization = Materialization::MissingNote {
                expected: "/notes/pwf/PWF-0001.md".to_string(),
            };

            let enriched = enrich(&rec, &project_path());

            assert!(!enriched.launchable, "missing note must not be launchable");
            assert!(enriched.needs_prompt, "empty prompt is a placeholder");
            assert_eq!(
                enriched.issues,
                [
                    "Task note missing: /notes/pwf/PWF-0001.md".to_string(),
                    ISSUE_PLACEHOLDER_PROMPT.to_string(),
                ]
            );
            assert_eq!(enriched.prompt, "");
            assert_eq!(enriched.format, "file");
            assert_eq!(
                enriched.task_file.as_deref(),
                Some("/notes/pwf/PWF-0001.md")
            );
        }

        #[test]
        fn empty_title_falls_back_to_id() {
            let mut rec = record("body");
            rec.title = "  ".to_string();
            assert_eq!(enrich(&rec, &project_path()).session, "PWF-0001");
        }

        #[test]
        fn section_label_is_normalized() {
            let mut rec = record("body");
            rec.section = Some("Futuro".to_string());
            assert_eq!(
                enrich(&rec, &project_path()).section.as_deref(),
                Some("Future")
            );
        }

        #[test]
        fn unknown_section_label_preserves_trimmed_case() {
            assert_eq!(normalize_section_label(" SomeDay "), "SomeDay");
        }

        #[test]
        fn derive_flags_orders_missing_note_before_placeholder() {
            let flags = derive_flags("", Some("/notes/pwf/PWF-0009.md"));
            assert_eq!(
                flags.issues,
                [
                    "Task note missing: /notes/pwf/PWF-0009.md".to_string(),
                    ISSUE_PLACEHOLDER_PROMPT.to_string(),
                ]
            );
        }
    }
}

pub(in crate::task) mod note_body {
    //! Applies prompt, lane, and report transforms to task note bodies.

    use std::sync::LazyLock;

    use prompt_lanes::{Adapter, MarkdownAdapter, parse};
    use regex::Regex;

    const REPORT_HEADER: &str = "### Report";
    const LANE_SECTION_HEADERS: [&str; 4] =
        ["## Goals", "## Context", "## Constraints", "## Done When"];

    static PLACEHOLDER_PROMPT_REGEX: LazyLock<Result<Regex, regex::Error>> = LazyLock::new(|| {
        Regex::new(r"(?i)(^\s*\[!\]\s*TODO\b|^\s*TODO\b|definir prompt|define prompt|tbd)")
    });

    enum PromptClassification {
        Placeholder,
        AuthoredVerbatimLegacy,
        Authored,
    }

    #[must_use]
    pub(in crate::task) fn render(prompt: &str) -> String {
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
    pub(in crate::task) fn is_placeholder_prompt(prompt: &str) -> bool {
        matches!(
            prompt_classification(prompt),
            PromptClassification::Placeholder
        )
    }

    fn prompt_classification(prompt: &str) -> PromptClassification {
        if prompt.trim().is_empty()
            || PLACEHOLDER_PROMPT_REGEX
                .as_ref()
                .is_ok_and(|regex| regex.is_match(prompt))
        {
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
    pub(in crate::task) fn append_lanes(body: &str, prompt: &str) -> Option<String> {
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
    pub(in crate::task) fn append_report_block(body: &str, report: &str) -> Option<String> {
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
    pub(in crate::task) fn append_report(body: &str, report: &str) -> Option<String> {
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

pub(in crate::task) mod prerequisite {
    use std::sync::LazyLock;

    use pwf_models::{
        project::Project,
        task::{PrerequisiteInput, PrerequisiteInputError, Prerequisites, TaskId, TaskStatus},
    };
    use regex::Regex;

    use crate::{
        ports::task_record::{Materialization, TaskRecord, TaskStore},
        task::dto::PrerequisiteStatus,
    };

    const PREREQUISITE_VALUE_PATTERN: &str = r"\[\[([A-Z]{3}-\d{4})";
    static PREREQUISITE_VALUE_REGEX: LazyLock<Result<Regex, regex::Error>> =
        LazyLock::new(|| Regex::new(PREREQUISITE_VALUE_PATTERN));
    static PERSISTED_STATUS_REGEX: LazyLock<Result<Regex, regex::Error>> =
        LazyLock::new(|| Regex::new(r"(?m)^status:\s*([^\r\n]+)$"));
    static PERSISTED_FRONTMATTER_REGEX: LazyLock<Result<Regex, regex::Error>> =
        LazyLock::new(|| {
            Regex::new(r"(?s)\A(?:\u{feff})?---[ \t]*\r?\n(.*?)\r?\n---[ \t]*(?:\r?\n|\z)")
        });

    fn prerequisite_value_regex() -> Option<&'static Regex> {
        PREREQUISITE_VALUE_REGEX.as_ref().ok()
    }

    #[derive(Debug, thiserror::Error)]
    pub(in crate::task) enum PrerequisiteValidationError {
        #[error("Unknown --prereq id(s): {}.", format_task_ids(ids))]
        UnknownIds { ids: Vec<TaskId> },
    }

    fn format_task_ids(ids: &[TaskId]) -> String {
        ids.iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    }

    pub(in crate::task) fn project_ids(
        values: &Prerequisites,
    ) -> Vec<pwf_models::project::ProjectId> {
        let mut projects = Vec::new();
        for id in values.iter() {
            let project = id.project_id();
            if !projects.contains(&project) {
                projects.push(project);
            }
        }
        projects
    }

    pub(in crate::task) fn referenced_project_ids<'a>(
        values: impl IntoIterator<Item = &'a str>,
    ) -> Vec<pwf_models::project::ProjectId> {
        let mut projects = Vec::new();
        let Some(regex) = prerequisite_value_regex() else {
            return projects;
        };
        for id in values
            .into_iter()
            .flat_map(|value| regex.captures_iter(value))
            .filter_map(|captures| TaskId::try_new(&captures[1]).ok())
        {
            let project = id.project_id();
            if !projects.contains(&project) {
                projects.push(project);
            }
        }
        projects
    }

    pub(in crate::task) fn extract(raw: &str) -> Option<Prerequisites> {
        let identifiers = prerequisite_value_regex()?
            .captures_iter(raw)
            .filter_map(|captures| TaskId::try_new(&captures[1]).ok())
            .collect();
        Prerequisites::try_new(identifiers).ok()
    }

    pub(in crate::task) fn parse_frontmatter(
        raw: &str,
    ) -> Result<Vec<TaskId>, PrerequisiteInputError> {
        let raw = raw.trim();
        let unquoted = raw
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
            .or_else(|| {
                raw.strip_prefix('\'')
                    .and_then(|value| value.strip_suffix('\''))
            })
            .unwrap_or(raw);
        let input = unquoted.parse::<PrerequisiteInput>()?;
        Ok(input.iter().cloned().collect())
    }

    pub(in crate::task) fn validate_and_merge(
        existing: Option<&str>,
        prerequisites: &Prerequisites,
        store: &impl TaskStore,
        projects: &[Project],
    ) -> Result<Prerequisites, PrerequisiteValidationError> {
        let mut unknown = Vec::new();
        for identifier in prerequisites.iter() {
            let Some(project) = projects
                .iter()
                .find(|project| project.id == identifier.project_id())
            else {
                unknown.push(identifier.clone());
                continue;
            };
            let Ok(record) = store.get(project, identifier) else {
                unknown.push(identifier.clone());
                continue;
            };
            if record.is_none_or(|record| !has_valid_persisted_status(&record)) {
                unknown.push(identifier.clone());
            }
        }
        if !unknown.is_empty() {
            return Err(PrerequisiteValidationError::UnknownIds { ids: unknown });
        }

        let mut identifiers = Vec::new();
        if let Some(regex) = prerequisite_value_regex() {
            for identifier in existing
                .into_iter()
                .flat_map(|value| regex.captures_iter(value))
                .filter_map(|captures| TaskId::try_new(&captures[1]).ok())
            {
                if !identifiers.contains(&identifier) {
                    identifiers.push(identifier);
                }
            }
        }
        for identifier in prerequisites.iter() {
            if !identifiers.contains(identifier) {
                identifiers.push(identifier.clone());
            }
        }
        match Prerequisites::try_new(identifiers) {
            Ok(merged) => Ok(merged),
            Err(_) => Ok(prerequisites.clone()),
        }
    }

    fn has_valid_persisted_status(record: &TaskRecord) -> bool {
        let (Ok(frontmatter_regex), Ok(status_regex)) = (
            PERSISTED_FRONTMATTER_REGEX.as_ref(),
            PERSISTED_STATUS_REGEX.as_ref(),
        ) else {
            return false;
        };
        matches!(record.materialization, Materialization::NoteFile)
            && frontmatter_regex
                .captures(&record.source)
                .and_then(|captures| captures.get(1))
                .and_then(|frontmatter| status_regex.captures(frontmatter.as_str()))
                .and_then(|captures| captures.get(1))
                .is_some_and(|value| value.as_str().trim().parse::<TaskStatus>().is_ok())
    }

    pub(in crate::task) fn statuses(
        prerequisites: &Prerequisites,
        store: &impl TaskStore,
        projects: &[Project],
    ) -> Vec<PrerequisiteStatus> {
        prerequisites
            .iter()
            .map(|id| {
                let status = status(store, projects, id);
                PrerequisiteStatus {
                    id: id.clone(),
                    status,
                }
            })
            .collect()
    }

    #[rustfmt::skip]
    fn status(
        store: &impl TaskStore,
        projects: &[Project],
        id: &TaskId,
    ) -> Option<TaskStatus> {
        let project = projects.iter().find(|project| project.id == id.project_id())?;
        // FIXME: Distinguish an absent prerequisite from a read or parse failure; both currently render as "missing" and can hide vault corruption.
        store
            .get(project, id)
            .ok()
            .flatten()
            .filter(|task| !matches!(&task.materialization, Materialization::MissingNote { .. }))
            .map(|task| task.status)
    }

    #[cfg(test)]
    mod tests {
        use pwf_models::task::{PrerequisiteInput, Prerequisites};

        use super::{PrerequisiteValidationError, referenced_project_ids, validate_and_merge};
        use crate::{
            ports::task_record::TaskRecord,
            task::resolve::testing::{staged, staged_ghost},
            testing::project,
        };

        fn inputs(values: &[&str]) -> Prerequisites {
            let inputs = values
                .iter()
                .map(|value| value.parse::<PrerequisiteInput>().unwrap())
                .collect::<Vec<_>>();
            Prerequisites::from_inputs(&inputs).unwrap()
        }

        #[test]
        fn persisted_values_expose_referenced_projects() {
            assert_eq!(
                referenced_project_ids(["[[CFG-0057]], [[PWF-0001]]", "[[CFG-0014]], ignored",])
                    .iter()
                    .map(AsRef::as_ref)
                    .collect::<Vec<_>>(),
                ["CFG", "PWF"]
            );
        }

        #[test]
        fn validator_parses_checks_existence_and_merges_first_seen_ids() {
            let (store, _registry) = staged();
            let projects = [project("PWF".parse().unwrap(), "pwf")];
            let merged = validate_and_merge(
                Some("[[PWF-0001]], [[PWF-0001]]"),
                &inputs(&["pwf1, PWF-0001"]),
                &store,
                &projects,
            )
            .unwrap();

            assert_eq!(merged.to_string(), "[[PWF-0001]]");
        }

        #[test]
        fn validator_preserves_prerequisite_diagnostics() {
            let (store, _registry) = staged();
            let projects = [project("PWF".parse().unwrap(), "pwf")];

            let unknown =
                validate_and_merge(None, &inputs(&["PWF-9999"]), &store, &projects).unwrap_err();
            assert!(matches!(
                unknown,
                PrerequisiteValidationError::UnknownIds { ref ids }
                    if ids.iter().map(AsRef::as_ref).collect::<Vec<_>>() == ["PWF-9999"]
            ));
            assert_eq!(unknown.to_string(), "Unknown --prereq id(s): PWF-9999.");
        }

        #[test]
        fn validator_rejects_missing_note_materializations_as_unknown() {
            let (store, _registry) = staged_ghost();
            let projects = [project("PWF".parse().unwrap(), "pwf")];

            let error =
                validate_and_merge(None, &inputs(&["PWF-0002"]), &store, &projects).unwrap_err();

            assert!(matches!(
                error,
                PrerequisiteValidationError::UnknownIds { ref ids }
                    if ids.iter().map(AsRef::as_ref).collect::<Vec<_>>() == ["PWF-0002"]
            ));
        }

        #[test]
        fn validator_rejects_missing_or_invalid_persisted_status_as_unknown() {
            let (staged_store, _registry) = staged();
            let projects = [project("PWF".parse().unwrap(), "pwf")];
            let base = staged_store.tasks("pwf")[0].clone();
            for source in [
                "---\nid: PWF-0001\ntitle: task\n---\n\nbody\n",
                "---\nid: PWF-0001\nstatus: paused\ntitle: task\n---\n\nbody\n",
                "---\nid: PWF-0001\ntitle: task\n---\n\nstatus: active\n",
            ] {
                let record = TaskRecord {
                    source: source.to_string(),
                    ..base.clone()
                };
                let store =
                    crate::testing::InMemoryStore::default().with_project("pwf", vec![record]);

                let error = validate_and_merge(None, &inputs(&["PWF-0001"]), &store, &projects)
                    .unwrap_err();

                assert!(matches!(
                    error,
                    PrerequisiteValidationError::UnknownIds { ref ids }
                        if ids.iter().map(AsRef::as_ref).collect::<Vec<_>>() == ["PWF-0001"]
                ));
            }
        }
    }
}

pub(in crate::task) mod resolve {
    use pwf_models::{project::Project, task::TaskId};

    use super::show_task::ShowTaskError;
    use crate::ports::task_record::{TaskRecord, TaskStore};

    pub(in crate::task) fn resolve_record(
        store: &impl TaskStore,
        project: &Project,
        id: &TaskId,
    ) -> Result<TaskRecord, ShowTaskError> {
        store
            .get(project, id)
            .map_err(|error| ShowTaskError::ReadStore(Box::new(error)))?
            .ok_or_else(|| ShowTaskError::TaskNotFound { id: id.clone() })
    }

    #[cfg(test)]
    pub(in crate::task) mod testing {
        use pwf_models::{
            project::Project,
            task::{TaskId, TaskStatus, Timestamp},
        };

        use crate::{
            ports::task_record::{Materialization, TaskRecord},
            testing::{InMemoryStore, project},
        };

        pub(in crate::task) const PWF_0001_SOURCE: &str = "---\nid: PWF-0001\nstatus: active\ntitle: do the thing\nproject: pwf\ncreated: 2026-06-20\n---\n\n## Goals\n- do the thing\n";

        pub(in crate::task) fn staged() -> (InMemoryStore, Vec<Project>) {
            let record = TaskRecord {
                id: TaskId::try_new("PWF-0001").unwrap(),
                title: "do the thing".to_string(),
                status: TaskStatus::Active,
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

        pub(in crate::task) fn staged_ghost() -> (InMemoryStore, Vec<Project>) {
            let record = TaskRecord {
                id: TaskId::try_new("PWF-0002").unwrap(),
                title: "ghost".to_string(),
                status: TaskStatus::Active,
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

        fn registry() -> Vec<Project> {
            vec![project("PWF".parse().unwrap(), "pwf")]
        }
    }

    #[cfg(test)]
    mod tests {
        use pwf_models::task::TaskId;

        use super::{
            resolve_record,
            testing::{staged, staged_ghost},
        };

        #[test]
        fn resolve_record_returns_path_and_markdown() {
            let (store, projects) = staged();

            let id = TaskId::try_new("PWF-0001").unwrap();
            let resolved = resolve_record(&store, &projects[0], &id).unwrap();

            assert_eq!(resolved.locator, "/notes/pwf/PWF-0001.md");
            assert_eq!(resolved.source, super::testing::PWF_0001_SOURCE);
        }

        #[test]
        fn resolve_returns_expected_note_path_for_missing_note_wikilink() {
            let (store, projects) = staged_ghost();

            let id = TaskId::try_new("PWF-0002").unwrap();
            let resolved = resolve_record(&store, &projects[0], &id).unwrap();

            assert_eq!(resolved.locator, "/notes/pwf/PWF-0002.md");
        }
    }
}

pub(in crate::task) mod section {
    #[must_use]
    pub(in crate::task) fn alias(label: &str) -> Option<&'static str> {
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

pub(in crate::task) mod store_util {
    //! Shared task loading and creation over the persistence ports.

    use pwf_models::{project::Project, task::TaskId};

    use super::{add_task::CreateTaskError, enrich::normalize_section_label};
    use crate::ports::task_record::{
        IndexEntry, IndexEntryState, IndexEntryStore, IndexSectionStore, NewTask, TaskRecord,
        TaskStore,
    };

    /// Reports failures while loading a required task.
    #[derive(Debug, thiserror::Error)]
    pub(in crate::task) enum LoadTaskError {
        #[error("Task not found: {id}")]
        TaskNotFound { id: TaskId },
        #[error("{0}")]
        Store(Box<dyn std::error::Error + Send + Sync>),
    }

    /// Labels that materialize as dedicated H2 sections.
    const SECTION_LABELS: [&str; 3] = ["Future", "Human", "Low-prio"];

    /// Contains a created record and the new H2 section, if one was needed.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(in crate::task) struct CreatedTask {
        pub(in crate::task) record: TaskRecord,
        pub(in crate::task) created_section: Option<String>,
    }

    /// Strips the frontmatter parser's retained leading blank line before a body rewrite.
    #[must_use]
    pub(in crate::task) fn body_region(body: &str) -> &str {
        body.strip_prefix('\n').unwrap_or(body)
    }

    /// Reads a required task within a project.
    pub(in crate::task) fn require_task(
        store: &impl TaskStore,
        project: &Project,
        id: &TaskId,
    ) -> Result<TaskRecord, LoadTaskError> {
        TaskStore::get(store, project, id)
            .map_err(|error| LoadTaskError::Store(Box::new(error)))?
            .ok_or_else(|| LoadTaskError::TaskNotFound { id: id.clone() })
    }

    /// Inserts a note record, then upserts its open index entry.
    ///
    /// # Panics
    ///
    /// Panics if the store's `insert` violates its contract by returning a record
    /// without a [`TaskId`].
    pub(in crate::task) fn create_task(
        store: &(impl TaskStore + IndexEntryStore + IndexSectionStore),
        project: &Project,
        new: NewTask,
    ) -> Result<CreatedTask, CreateTaskError> {
        let target_section = new.section.clone();
        // Read sections before writing so an invalid index leaves no orphaned note.
        let existing = IndexSectionStore::list_index_sections(store, project)
            .map_err(|error| CreateTaskError::ReadSections(Box::new(error)))?;
        let created_section = target_section
            .as_deref()
            .filter(|label| SECTION_LABELS.contains(label))
            .filter(|label| {
                !existing
                    .iter()
                    .any(|section| normalize_section_label(&section.label) == *label)
            })
            .map(str::to_string);

        let record = TaskStore::insert(store, project, new)
            .map_err(|error| CreateTaskError::InsertRecord(Box::new(error)))?;
        let id = record.id.clone();
        IndexEntryStore::upsert_index_entry(
            store,
            project,
            IndexEntry {
                id,
                state: IndexEntryState::Open,
                // Preserve the raw label; the adapter owns placement.
                section: target_section.unwrap_or_default(),
            },
        )
        .map_err(|error| CreateTaskError::InsertIndex {
            project: project.title.clone(),
            created_section: created_section.clone(),
            source: Box::new(error),
        })?;

        Ok(CreatedTask {
            record,
            created_section,
        })
    }

    #[cfg(test)]
    mod tests {
        use pwf_models::{
            project::Project,
            task::{TaskId, TaskTitle, Timestamp},
        };

        use super::{LoadTaskError, create_task, require_task};
        use crate::{
            ports::task_record::{IndexEntryState, NewTask},
            task::resolve::testing::staged,
            testing::{InMemoryStore, project},
        };

        fn pwf() -> Project {
            project("PWF".parse().unwrap(), "pwf")
        }

        fn new_task(section: Option<&str>) -> NewTask {
            NewTask {
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
            InMemoryStore::default().with_project_id("pwf", "PWF".parse().unwrap())
        }

        #[test]
        fn create_task_inserts_record_and_open_index_entry() {
            let store = staged_store();

            let created = create_task(&store, &pwf(), new_task(None)).unwrap();

            let id = TaskId::try_new("PWF-0001").unwrap();
            assert_eq!(created.record.id, id.clone());
            assert_eq!(created.created_section, None);
            let tasks = store.tasks("pwf");
            assert_eq!(tasks.len(), 1, "record must be inserted");
            assert_eq!(tasks[0].id, id.clone());
            let entries = store.entries("pwf");
            assert_eq!(entries.len(), 1, "open index entry must be upserted");
            assert_eq!(entries[0].id, id);
            assert_eq!(entries[0].state, IndexEntryState::Open);
            assert_eq!(entries[0].section, "");
        }

        #[test]
        fn create_task_reports_created_section_when_region_absent() {
            let store = staged_store();

            let created = create_task(&store, &pwf(), new_task(Some("Human"))).unwrap();

            assert_eq!(created.created_section.as_deref(), Some("Human"));
            assert_eq!(store.entries("pwf")[0].section, "Human");
        }

        #[test]
        fn create_task_does_not_report_existing_empty_section_region() {
            let store = staged_store().with_sections("pwf", &["Human"]);

            let created = create_task(&store, &pwf(), new_task(Some("Human"))).unwrap();

            assert_eq!(created.created_section, None);
        }

        #[test]
        fn create_task_matches_section_aliases_like_the_legacy_read_headers() {
            let store = staged_store().with_sections("pwf", &["Futuro"]);

            let created = create_task(&store, &pwf(), new_task(Some("Future"))).unwrap();

            assert_eq!(created.created_section, None);
        }

        #[test]
        fn require_task_returns_staged_record() {
            let (store, _registry) = staged();
            let id = TaskId::try_new("PWF-0001").unwrap();

            let record = require_task(&store, &pwf(), &id).unwrap();

            assert_eq!(record.id, id);
        }

        #[test]
        fn require_task_missing_reports_the_typed_id() {
            let (store, _registry) = staged();
            let id = TaskId::try_new("PWF-9999").unwrap();

            let error = require_task(&store, &pwf(), &id).unwrap_err();

            assert!(matches!(
                error,
                LoadTaskError::TaskNotFound { ref id } if id.as_ref() == "PWF-9999"
            ));
            assert_eq!(error.to_string(), "Task not found: PWF-9999");
        }

        #[test]
        fn created_item_carries_record_and_section_fact() {
            let store = staged_store();
            let created = create_task(&store, &pwf(), new_task(Some("Low-prio"))).unwrap();
            assert_eq!(created.record.id, TaskId::try_new("PWF-0001").unwrap());
            assert_eq!(created.record.title, "ship it");
            assert_eq!(created.created_section.as_deref(), Some("Low-prio"));
        }
    }
}

pub(in crate::task) mod tag_policy {
    use pwf_models::task::{Tag, Tags};

    pub(in crate::task) fn parse_values(values: &[String]) -> Result<Tags, ParseTagsError> {
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

    pub(in crate::task) fn parse_frontmatter(raw: &str) -> Result<Tags, ParseTagsError> {
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
    pub(in crate::task) fn merge(existing: &Tags, appended: &Tags) -> Tags {
        let mut merged = existing.iter().cloned().collect::<Vec<_>>();
        for tag in appended.iter() {
            if !merged.contains(tag) {
                merged.push(tag.clone());
            }
        }
        match Tags::try_new(merged) {
            Ok(tags) => tags,
            Err(_) => existing.clone(),
        }
    }

    #[must_use]
    pub(in crate::task) fn contains_all(stored: &Tags, requested: &Tags) -> bool {
        requested
            .iter()
            .all(|requested| stored.iter().any(|stored| stored == requested))
    }

    #[must_use]
    #[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
    pub(in crate::task) enum ParseTagsError {
        #[error("missing tag value: {raw:?}")]
        MissingTag { raw: String },
        #[error("invalid tag value: {raw:?}")]
        InvalidTag { raw: String },
        #[error("invalid tags frontmatter: {raw:?}")]
        InvalidFrontmatter { raw: String },
    }

    impl ParseTagsError {
        pub(in crate::task) fn raw(&self) -> &str {
            match self {
                Self::MissingTag { raw }
                | Self::InvalidTag { raw }
                | Self::InvalidFrontmatter { raw } => raw,
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use pwf_models::task::Tags;

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

pub(in crate::task) mod title {
    use prompt_lanes::parse;
    use pwf_models::task::{TaskTitle, TaskTitleError};

    pub(in crate::task) fn inferred(prompt: &str) -> Result<TaskTitle, TaskTitleError> {
        TaskTitle::try_new(parse(prompt).title)
    }
}

pub(in crate::task) mod task_update {
    use pwf_models::{
        project::Project,
        task::{Tags, TaskId, TaskStatus},
    };

    use crate::{
        ports::task_record::{TaskPatch, TaskRecord, TaskStore},
        task::{
            dto::{PreparedTaskUpdate, TaskIdentity},
            logic::{
                commit_provenance,
                note_body::{append_lanes, append_report_block, render},
                prerequisite::{PrerequisiteValidationError, validate_and_merge},
                store_util::body_region,
                tag_policy,
            },
            update_task::{UpdateTask, UpdateTaskError, UpdateTaskOk},
        },
    };
    pub(in crate::task) fn prepare(
        command: &UpdateTask,
        store: &impl TaskStore,
        project: &Project,
        projects: &[Project],
    ) -> Result<PreparedTaskUpdate, UpdateTaskError> {
        let commits = commit_provenance::normalize(&command.commits);
        let tags = parse_tags(&command.tags)?;
        let edits_body = command.prompt.is_some()
            || command.title.is_some()
            || command.prereq.is_some()
            || command.clear_prereq
            || command.append.is_some()
            || command.effort.is_some()
            || tags.is_some()
            || command.tags_clear;
        if !edits_body && commits.is_none() && command.append_report.is_none() {
            return Err(UpdateTaskError::NothingToUpdate);
        }

        let not_found = || UpdateTaskError::TaskNotFound {
            id: command.id.clone(),
        };
        let record = store
            .get(project, &command.id)
            .map_err(|error| UpdateTaskError::WriteStore(Box::new(error)))?
            .ok_or_else(not_found)?;
        let identity = TaskIdentity {
            project: project.clone(),
            identifier: command.id.clone(),
        };

        if record.status == TaskStatus::Active {
            prepare_open_task(
                store,
                projects,
                identity,
                command,
                commits.as_deref(),
                tags.as_ref(),
                &record,
            )
        } else if edits_body {
            Err(UpdateTaskError::ClosedTaskAmendOnly {
                id: command.id.clone(),
            })
        } else {
            amend_closed_task(identity, command, commits.as_deref(), &record)
        }
    }

    pub(in crate::task) fn persist(
        prepared: PreparedTaskUpdate,
        store: &impl TaskStore,
    ) -> Result<UpdateTaskOk, UpdateTaskError> {
        store
            .update(
                &prepared.identity.project,
                &prepared.identity.identifier,
                prepared.patch,
            )
            .map_err(|error| UpdateTaskError::WriteStore(Box::new(error)))?;
        Ok(prepared.outcome)
    }

    fn prepare_open_task(
        store: &impl TaskStore,
        projects: &[Project],
        identity: TaskIdentity,
        command: &UpdateTask,
        commits: Option<&str>,
        tags: Option<&Tags>,
        record: &TaskRecord,
    ) -> Result<PreparedTaskUpdate, UpdateTaskError> {
        let new_title = command
            .title
            .as_ref()
            .map_or_else(|| record.title.clone(), ToString::to_string);

        let mut patch = TaskPatch {
            body: compute_body(command, record)?,
            ..TaskPatch::default()
        };
        if command.title.is_some() {
            patch.title.clone_from(&command.title);
        }
        if command.clear_prereq {
            patch.prereq = Some(None);
        } else if let Some(prerequisites) = command.prereq.as_ref() {
            let merged =
                validate_and_merge(record.prereq.as_deref(), prerequisites, store, projects)
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

        let outcome = UpdateTaskOk::OpenTaskEdit {
            id: identity.identifier.clone(),
            project: identity.project.title.to_string(),
            title: new_title,
        };
        Ok(PreparedTaskUpdate {
            identity,
            patch,
            outcome,
        })
    }

    /// Computes a replacement body when a prompt, lane, or report edit requires one.
    ///
    /// Prompt replacement starts from a new body; lane and report edits build on that result.
    fn compute_body(
        command: &UpdateTask,
        record: &TaskRecord,
    ) -> Result<Option<String>, UpdateTaskError> {
        let base = body_region(&record.body);
        let mut body: Option<String> = command.prompt.as_deref().map(render);
        if let Some(append) = &command.append {
            let current = body.as_deref().unwrap_or(base);
            body = Some(append_lanes(current, append).ok_or(UpdateTaskError::EmptyAppend)?);
        }
        if let Some(report) = &command.append_report {
            let current = body.as_deref().unwrap_or(base);
            body = Some(append_report_block(current, report).ok_or(UpdateTaskError::EmptyReport)?);
        }
        Ok(body)
    }

    /// Resolves the tri-state `TaskPatch.tags` value: unchanged, cleared, or replaced.
    #[expect(
        clippy::option_option,
        reason = "preserves TaskPatch.tags tri-state semantics"
    )]
    fn resolve_tags(
        appended: Option<&Tags>,
        clear: bool,
        id: &TaskId,
        record: &TaskRecord,
    ) -> Result<Option<Option<Tags>>, UpdateTaskError> {
        let Some(appended) = appended else {
            return Ok(clear.then_some(None));
        };
        let tags = if clear {
            appended.clone()
        } else if let Some(existing) = record.tags.as_deref() {
            let existing = tag_policy::parse_frontmatter(existing).map_err(|error| {
                UpdateTaskError::InvalidTagsFrontmatter {
                    id: id.clone(),
                    raw: error.raw().to_string(),
                }
            })?;
            tag_policy::merge(&existing, appended)
        } else {
            appended.clone()
        };
        Ok(Some(Some(tags)))
    }

    fn amend_closed_task(
        identity: TaskIdentity,
        command: &UpdateTask,
        commits: Option<&str>,
        record: &TaskRecord,
    ) -> Result<PreparedTaskUpdate, UpdateTaskError> {
        let mut patch = TaskPatch::default();
        let mut changes = Vec::new();
        if let Some(commits) = commits {
            patch.commits = Some(Some(commits.to_string()));
            changes.push(commits_change(commits));
        }
        if let Some(report) = &command.append_report {
            let base = body_region(&record.body);
            patch.body =
                Some(append_report_block(base, report).ok_or(UpdateTaskError::EmptyReport)?);
            changes.push("report appended".to_string());
        }
        let outcome = UpdateTaskOk::Changed {
            id: identity.identifier.clone(),
            changes,
        };
        Ok(PreparedTaskUpdate {
            identity,
            patch,
            outcome,
        })
    }

    fn commits_change(commits: &str) -> String {
        format!("commits: {commits}")
    }

    fn map_prerequisite_error(error: PrerequisiteValidationError) -> UpdateTaskError {
        match error {
            PrerequisiteValidationError::UnknownIds { ids } => {
                UpdateTaskError::UnknownPrereqIds { ids }
            }
        }
    }

    fn parse_tags(values: &[String]) -> Result<Option<Tags>, UpdateTaskError> {
        if values.is_empty() {
            return Ok(None);
        }
        tag_policy::parse_values(values)
            .map(Some)
            .map_err(|error| UpdateTaskError::InvalidTag {
                raw: error.raw().to_string(),
            })
    }
}

pub(in crate::task) mod task_creation {
    use std::path::PathBuf;

    use pwf_models::project::Project;

    use crate::task::{add_task::AddTaskOk, logic::store_util};

    pub(in crate::task) fn project_mapped(project: &Project) -> Project {
        project.clone()
    }

    pub(in crate::task) fn added_task(
        project: &Project,
        created: store_util::CreatedTask,
    ) -> AddTaskOk {
        let id = created.record.id.clone();
        AddTaskOk {
            id,
            project: project.title.to_string(),
            title: created.record.title,
            note_path: PathBuf::from(created.record.locator),
            created_section: created.created_section,
        }
    }
}

pub(in crate::task) mod task_closing {
    use pwf_models::{
        project::ProjectId,
        task::{TaskId, TaskStatus, Timestamp},
    };

    use crate::{
        ports::task_record::{
            IndexEntry, IndexEntryState, IndexEntryStore, IndexSection, IndexSectionStore,
            Materialization, NewTask, TaskPatch, TaskStore,
        },
        task::{
            add_task::{AddTaskError, AddTaskOk, TaskSection},
            complete_task::{ClosedTaskAction, CompleteTaskError, CompleteTaskOk},
            logic::{
                commit_provenance,
                note_body::append_report,
                store_util::{self, LoadTaskError, body_region},
                task_creation::{added_task, project_mapped},
                title,
            },
        },
    };

    mod queue {
        use pwf_models::task::{TaskId, Timestamp};

        use crate::{
            ports::task_record::{IndexEntry, IndexEntryState, IndexSection},
            task::section,
        };

        const SECTION_CAPS: &[(&str, usize)] =
            &[("General", 6), ("Low-prio", 3), ("Future", 3), ("Human", 3)];

        pub(super) struct CloseDecisions {
            pub(super) evicted_ids: Vec<TaskId>,
            pub(super) normalize_futuro_header: bool,
            pub(super) mark_target: bool,
        }

        pub(super) fn close_decisions(
            entries: &[IndexEntry],
            sections: &[IndexSection],
            id: &TaskId,
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
            id: &TaskId,
            completed: &Timestamp,
        ) -> Vec<TaskId> {
            let target_section = normalize_section(target_section);
            let Some(cap) = section_cap(&target_section) else {
                return Vec::new();
            };

            let mut done: Vec<(&str, &TaskId)> = entries
                .iter()
                .filter_map(|entry| match &entry.state {
                    IndexEntryState::Done(entry_completed)
                        if normalize_section(&entry.section) == target_section =>
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

        fn section_cap(normalized_section: &str) -> Option<usize> {
            SECTION_CAPS
                .iter()
                .find(|(name, _)| *name == normalized_section)
                .map(|(_, cap)| *cap)
        }

        fn normalize_section(label: &str) -> String {
            if label.trim().is_empty() || label.trim() == "General" {
                return "General".to_string();
            }
            section::alias(label).map_or_else(|| label.trim().to_lowercase(), str::to_string)
        }

        #[cfg(test)]
        mod tests {
            use super::*;

            fn id(raw: &str) -> TaskId {
                TaskId::try_new(raw).unwrap()
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
    pub(in crate::task) struct CloseTaskRequest<'a> {
        pub(in crate::task) action: ClosedTaskAction,
        pub(in crate::task) id: &'a TaskId,
        pub(in crate::task) completed: Timestamp,
        pub(in crate::task) report: Option<&'a str>,
        pub(in crate::task) commits: &'a [String],
        pub(in crate::task) review: bool,
    }

    pub(in crate::task) enum CloseError {
        TaskNotFound {
            id: TaskId,
        },
        UnknownProjectId {
            task_id: TaskId,
            project_id: ProjectId,
        },
        EmptyReport,
        WriteStore(Box<dyn std::error::Error + Send + Sync>),
        ReviewTask(AddTaskError),
    }

    impl CloseError {
        pub(in crate::task) fn into_complete(self) -> CompleteTaskError {
            match self {
                Self::TaskNotFound { id } => CompleteTaskError::TaskNotFound { id },
                Self::UnknownProjectId {
                    task_id,
                    project_id,
                } => CompleteTaskError::UnknownProjectId {
                    task_id,
                    project_id,
                },
                Self::EmptyReport => CompleteTaskError::EmptyReport,
                Self::WriteStore(source) => CompleteTaskError::WriteStore(source),
                Self::ReviewTask(source) => CompleteTaskError::ReviewTask(source),
            }
        }
    }

    /// Closes an task through the flow shared by done and cancel.
    ///
    /// One patch applies report and status fields. Only note-backed records rotate the queue,
    /// because missing-note records have no file-backed queue entry.
    pub(in crate::task) fn perform_close(
        store: &(impl TaskStore + IndexEntryStore + IndexSectionStore),
        project: &pwf_models::project::Project,
        request: CloseTaskRequest<'_>,
    ) -> Result<CompleteTaskOk, CloseError> {
        let CloseTaskRequest {
            action,
            id,
            completed,
            report,
            commits,
            review,
        } = request;
        let commits_value = commit_provenance::normalize(commits);
        let task_identifier = id.clone();
        if project.id != task_identifier.project_id() {
            return Err(CloseError::UnknownProjectId {
                project_id: task_identifier.project_id(),
                task_id: task_identifier,
            });
        }
        let record =
            store_util::require_task(store, project, &task_identifier).map_err(map_load)?;
        if record.status != TaskStatus::Active {
            return Err(CloseError::TaskNotFound {
                id: task_identifier,
            });
        }
        let title = record.title.clone();
        let mut patch = TaskPatch {
            status: Some(action.status()),
            completed: Some(Some(completed.clone())),
            ..TaskPatch::default()
        };
        if let Some(report) = report {
            let body =
                append_report(body_region(&record.body), report).ok_or(CloseError::EmptyReport)?;
            patch.body = Some(body);
        }
        if let Some(commits) = &commits_value {
            patch.commits = Some(Some(commits.clone()));
        }
        TaskStore::update(store, project, &task_identifier, patch)
            .map_err(|error| CloseError::WriteStore(Box::new(error)))?;

        let (evicted_ids, futuro_renamed) =
            if matches!(record.materialization, Materialization::NoteFile) {
                rotate_done_queue(store, project, &task_identifier, completed.as_str())?
            } else {
                (Vec::new(), false)
            };

        let review_task = review
            .then(|| {
                spawn_review(
                    store,
                    project,
                    &task_identifier,
                    completed.as_str(),
                    commits_value.as_deref(),
                )
            })
            .transpose()?;

        Ok(CompleteTaskOk {
            id: task_identifier,
            project: project.title.clone(),
            title,
            action,
            evicted_ids,
            futuro_renamed_project: futuro_renamed.then(|| project.title.clone()),
            review_task,
        })
    }

    fn map_load(error: LoadTaskError) -> CloseError {
        match error {
            LoadTaskError::TaskNotFound { id } => CloseError::TaskNotFound { id },
            LoadTaskError::Store(source) => CloseError::WriteStore(source),
        }
    }

    /// Applies header normalization, the closed entry, and cap-based evictions to the index.
    fn rotate_done_queue(
        store: &(impl IndexEntryStore + IndexSectionStore),
        project: &pwf_models::project::Project,
        id: &TaskId,
        completed: &str,
    ) -> Result<(Vec<TaskId>, bool), CloseError> {
        let entries = IndexEntryStore::list_index_entries(store, project)
            .map_err(|error| CloseError::WriteStore(Box::new(error)))?;
        let sections = IndexSectionStore::list_index_sections(store, project)
            .map_err(|error| CloseError::WriteStore(Box::new(error)))?;
        let completed = Timestamp::new(completed);
        let decisions = close_decisions(&entries, &sections, id, &completed);

        if decisions.normalize_futuro_header {
            rename_futuro_headers(store, project, &sections)?;
        }
        if decisions.mark_target {
            IndexEntryStore::upsert_index_entry(
                store,
                project,
                IndexEntry {
                    id: id.clone(),
                    state: IndexEntryState::Done(completed),
                    section: String::new(),
                },
            )
            .map_err(|error| CloseError::WriteStore(Box::new(error)))?;
        }
        for evicted in &decisions.evicted_ids {
            IndexEntryStore::delete_index_entry(store, project, evicted)
                .map_err(|error| CloseError::WriteStore(Box::new(error)))?;
        }
        Ok((decisions.evicted_ids, decisions.normalize_futuro_header))
    }

    /// Renames every `## Futuro` header to `## Future` through the section port.
    fn rename_futuro_headers(
        store: &impl IndexSectionStore,
        project: &pwf_models::project::Project,
        sections: &[IndexSection],
    ) -> Result<(), CloseError> {
        for section in sections.iter().filter(|s| is_futuro_label(&s.label)) {
            IndexSectionStore::rename_index_section(store, project, &section.label, "Future")
                .map_err(|error| CloseError::WriteStore(Box::new(error)))?;
        }
        Ok(())
    }

    fn spawn_review(
        store: &(impl TaskStore + IndexEntryStore + IndexSectionStore),
        project: &pwf_models::project::Project,
        reviewed: &TaskId,
        completed: &str,
        commits: Option<&str>,
    ) -> Result<AddTaskOk, CloseError> {
        let project = project_mapped(project);
        let prompt = review_task_prompt(reviewed, commits);
        let review_title = title::inferred(&prompt)
            .map_err(AddTaskError::from)
            .map_err(CloseError::ReviewTask)?;
        let created = store_util::create_task(
            store,
            &project,
            NewTask {
                title: review_title,
                prompt,
                created: Timestamp::new(completed),
                section: Some(TaskSection::Human.as_str().to_string()),
                prereq: None,
                effort: None,
                tags: None,
            },
        )
        .map_err(|source| {
            CloseError::ReviewTask(AddTaskError::WriteStore {
                diagnostics: crate::task::add_task::AddTaskDiagnostics {
                    project: project.title.to_string(),
                    created_section: source
                        .created_section()
                        .map(|(_, section)| section.to_string()),
                },
                source,
            })
        })?;
        Ok(added_task(&project, created))
    }

    pub(in crate::task) fn review_task_prompt(reviewed_id: &TaskId, range: Option<&str>) -> String {
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
