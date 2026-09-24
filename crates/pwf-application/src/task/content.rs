//! Applies body, section, and report transforms to task file bodies.

use std::fmt::Write as _;

use lazy_regex::{Regex, regex};
use pulldown_cmark::{Event, Parser, Tag};
use pwf_models::task::TaskBody;

use super::body_presets::{MarkerSectionDefinition, MarkerSectionItemStyle, TaskBodyPreset};

const REPORT_SEPARATOR: &str = "\n\n### Report\n\n";

fn placeholder_body_regex() -> &'static Regex {
    regex!(r"(?i)(^\s*\[!\]\s*TODO\b|^\s*TODO\b|definir prompt|define prompt|tbd)")
}

enum BodyClassification {
    Placeholder,
    AuthoredVerbatimLegacy,
    Authored,
}

#[derive(Clone, Copy)]
enum ExplicitEmptySections {
    Render,
    Omit,
}

#[must_use]
pub(in crate::task) fn render_for_creation(body: &str, preset: &TaskBodyPreset) -> String {
    render_shorthand(body, preset, ExplicitEmptySections::Render)
}

#[must_use]
pub(in crate::task) fn render_for_replacement(body: &str, preset: &TaskBodyPreset) -> String {
    render_shorthand(body, preset, ExplicitEmptySections::Omit)
}

fn render_shorthand(
    body: &str,
    preset: &TaskBodyPreset,
    explicit_empty_sections: ExplicitEmptySections,
) -> String {
    match body_classification(body) {
        BodyClassification::Placeholder | BodyClassification::AuthoredVerbatimLegacy => {
            body.to_string()
        }
        BodyClassification::Authored => {
            let parsed = preset.parse(body);
            match explicit_empty_sections {
                ExplicitEmptySections::Render => preset.render(&parsed),
                ExplicitEmptySections::Omit => preset.render(&parsed.omit_empty_sections()),
            }
        }
    }
}

/// Reports whether a body is empty or matches `TODO`, `[!] TODO`, the legacy `define prompt` or
/// `definir prompt` phrases, or `tbd` case-insensitively.
#[must_use]
pub(in crate::task) fn is_placeholder_body(body: &TaskBody) -> bool {
    matches!(
        body_classification(body.as_ref()),
        BodyClassification::Placeholder
    )
}

fn body_classification(body: &str) -> BodyClassification {
    if body.trim().is_empty() || placeholder_body_regex().is_match(body) {
        return BodyClassification::Placeholder;
    }

    // Rendering uses ASCII boundaries, but diagnostics use Unicode boundaries.
    let body_lowercase = body.to_lowercase();
    let body_trimmed = body_lowercase.trim_start();
    let body_after_marker = body_trimmed.strip_prefix("[!]").map(str::trim_start);
    if [Some(body_trimmed), body_after_marker]
        .into_iter()
        .flatten()
        .any(starts_with_todo_word_boundary_ascii)
    {
        BodyClassification::AuthoredVerbatimLegacy
    } else {
        BodyClassification::Authored
    }
}

fn starts_with_todo_word_boundary_ascii(text: &str) -> bool {
    text.strip_prefix("todo").is_some_and(|rest| {
        !rest.starts_with(|character: char| character.is_ascii_alphanumeric() || character == '_')
    })
}

/// Splices shorthand items into existing sections and appends missing sections.
///
/// Items continue the style of a section's last block; empty and new sections use the preset.
#[must_use]
pub(in crate::task) fn append_marker_sections(
    body: &str,
    shorthand: &str,
    preset: &TaskBodyPreset,
) -> String {
    let shorthand = shorthand.trim();
    debug_assert!(!shorthand.is_empty(), "task append bodies are validated");
    let (title, mut sections) = preset.parse(shorthand).into_parts();
    if !title.is_empty() {
        sections[0].insert(0, title);
    }
    let mut out = body.to_string();
    for (section, items) in preset.sections().iter().zip(&sections) {
        out = append_items_to_section(&out, section, items);
    }
    out
}

fn header_bounds<'a>(
    content: &'a str,
    header: &'a str,
) -> impl Iterator<Item = (usize, usize)> + 'a {
    heading_bounds(content)
        .filter(move |&(start, end)| atx_heading_text(&content[start..end]) == Some(header))
}

/// Returns the trimmed text of an ATX heading line at any level.
fn atx_heading_text(line: &str) -> Option<&str> {
    let text = line.trim_start_matches('#');
    let level = line.len() - text.len();
    if !(1..=6).contains(&level) {
        return None;
    }
    text.strip_prefix([' ', '\t']).map(str::trim)
}

fn heading_bounds(content: &str) -> impl Iterator<Item = (usize, usize)> + '_ {
    let mut depth = 0;
    Parser::new(content)
        .into_offset_iter()
        .filter_map(move |(event, range)| match event {
            Event::Start(tag) => {
                let heading = depth == 0 && matches!(tag, Tag::Heading { .. });
                depth += 1;
                heading.then(|| {
                    let end =
                        range.start + content[range.clone()].find('\n').unwrap_or(range.len());
                    (range.start, end)
                })
            }
            Event::End(_) => {
                depth -= 1;
                None
            }
            _ => None,
        })
}

fn section_end(content: &str, header_end: usize) -> usize {
    let rest = &content[header_end..];
    header_end
        + heading_bounds(rest)
            .next()
            .map_or(rest.len(), |(start, _)| start)
}

fn append_items_to_section(
    content: &str,
    section: &MarkerSectionDefinition,
    items: &[String],
) -> String {
    if items.is_empty() {
        return content.to_string();
    }
    let Some((_, header_end)) = header_bounds(content, section.header()).next() else {
        let mut out = content.trim_end().to_string();
        out.push_str("\n\n");
        for _ in 0..section.heading_level().get() {
            out.push('#');
        }
        out.push(' ');
        out.push_str(section.header());
        append_items(
            &mut out,
            ItemContinuation::configured(section.item_style()),
            items,
            true,
        );
        out.push('\n');
        return out;
    };
    let section_end = section_end(content, header_end);
    let before = content[..section_end].trim_end_matches('\n');
    let after = content[section_end..].trim_start_matches('\n');
    let (continuation, starts_new_block) =
        ItemContinuation::detect(&content[header_end..section_end], section.item_style());
    let mut out = before.to_string();
    append_items(&mut out, continuation, items, starts_new_block);
    if after.is_empty() {
        out.push('\n');
    } else {
        out.push_str("\n\n");
        out.push_str(after);
    }
    out
}

/// Describes how appended items continue a section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ItemContinuation {
    Bullet { marker: char },
    Numbered { next_number: u64, delimiter: char },
    Paragraph,
}

impl ItemContinuation {
    const fn configured(item_style: MarkerSectionItemStyle) -> Self {
        match item_style {
            MarkerSectionItemStyle::Bullet => Self::Bullet { marker: '-' },
            MarkerSectionItemStyle::Numbered => Self::Numbered {
                next_number: 1,
                delimiter: '.',
            },
            MarkerSectionItemStyle::Paragraph => Self::Paragraph,
        }
    }

    /// Follows the section's last top-level list or paragraph, or the configured style when it
    /// has neither. The returned flag reports whether appended items start a new block.
    fn detect(section_content: &str, item_style: MarkerSectionItemStyle) -> (Self, bool) {
        let scan = ItemBlockScan::scan(section_content);
        let list_item_source = |first_item_start: Option<usize>| {
            first_item_start.map_or("", |start| section_content[start..].trim_start())
        };
        let continuation = match scan.last_item_block {
            None => Self::configured(item_style),
            Some(ItemBlock::Paragraph) => Self::Paragraph,
            Some(ItemBlock::List {
                first_number: None,
                first_item_start,
                ..
            }) => Self::Bullet {
                marker: list_item_source(first_item_start)
                    .chars()
                    .next()
                    .unwrap_or('-'),
            },
            Some(ItemBlock::List {
                first_number: Some(first_number),
                item_count,
                first_item_start,
            }) => Self::Numbered {
                next_number: first_number + item_count,
                delimiter: list_item_source(first_item_start)
                    .trim_start_matches(|character: char| character.is_ascii_digit())
                    .chars()
                    .next()
                    .unwrap_or('.'),
            },
        };
        (continuation, !scan.item_block_is_last)
    }
}

/// Describes one top-level block that can hold section items.
enum ItemBlock {
    Paragraph,
    List {
        first_number: Option<u64>,
        item_count: u64,
        first_item_start: Option<usize>,
    },
}

/// Records a section's last top-level list or paragraph and whether any other block follows it.
struct ItemBlockScan {
    last_item_block: Option<ItemBlock>,
    item_block_is_last: bool,
}

impl ItemBlockScan {
    fn scan(section_content: &str) -> Self {
        let mut scan = Self {
            last_item_block: None,
            item_block_is_last: false,
        };
        let mut depth = 0_usize;
        for (event, range) in Parser::new(section_content).into_offset_iter() {
            match event {
                Event::Start(tag) => {
                    scan.observe_start(depth, &tag, range.start);
                    depth += 1;
                }
                Event::End(_) => depth -= 1,
                _ => {}
            }
        }
        scan
    }

    fn observe_start(&mut self, depth: usize, tag: &Tag<'_>, start: usize) {
        match (depth, tag) {
            (0, Tag::Paragraph) => {
                self.last_item_block = Some(ItemBlock::Paragraph);
                self.item_block_is_last = true;
            }
            (0, Tag::List(first_number)) => {
                self.last_item_block = Some(ItemBlock::List {
                    first_number: *first_number,
                    item_count: 0,
                    first_item_start: None,
                });
                self.item_block_is_last = true;
            }
            (0, _) => self.item_block_is_last = false,
            (1, Tag::Item) => {
                if let Some(ItemBlock::List {
                    item_count,
                    first_item_start,
                    ..
                }) = &mut self.last_item_block
                {
                    *item_count += 1;
                    first_item_start.get_or_insert(start);
                }
            }
            _ => {}
        }
    }
}

fn append_items(
    out: &mut String,
    continuation: ItemContinuation,
    items: &[String],
    starts_new_block: bool,
) {
    for (index, item) in (0_u64..).zip(items) {
        let list_separator = if index == 0 && starts_new_block {
            "\n\n"
        } else {
            "\n"
        };
        match continuation {
            ItemContinuation::Bullet { marker } => {
                out.push_str(list_separator);
                out.push(marker);
                out.push(' ');
            }
            ItemContinuation::Numbered {
                next_number,
                delimiter,
            } => {
                let _ = write!(out, "{list_separator}{}{delimiter} ", next_number + index);
            }
            ItemContinuation::Paragraph => out.push_str("\n\n"),
        }
        out.push_str(item);
    }
}

/// Appends a whitespace-collapsed report under a new `### Report` heading.
#[must_use]
pub(in crate::task) fn append_report(body: &str, report: &str) -> String {
    let mut out = body.trim_end().to_string();
    out.push_str(REPORT_SEPARATOR);
    out.push_str(report);
    out.push('\n');
    out
}

/// Removes the last completion report appended by [`append_report`].
#[must_use]
pub(in crate::task) fn remove_report(body: &str) -> (String, Option<String>) {
    body.rsplit_once(REPORT_SEPARATOR).map_or_else(
        || (body.to_string(), None),
        |(body, report)| {
            (
                body.trim_end().to_string(),
                Some(report.trim_end().to_string()),
            )
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: &str = "\n\n";

    fn preset(name: &str) -> TaskBodyPreset {
        super::super::body_presets::TaskBodyPresets::try_new(Vec::new(), Some(name), Vec::new())
            .unwrap()
            .selected()
            .clone()
    }

    fn custom_preset(sections: &[(&str, &str, u8, MarkerSectionItemStyle)]) -> TaskBodyPreset {
        TaskBodyPreset::try_new(
            "custom",
            sections
                .iter()
                .map(|(marker, header, heading_level, item_style)| {
                    MarkerSectionDefinition::try_new(
                        *marker,
                        *header,
                        super::super::body_presets::MarkerSectionHeadingLevel::try_new(
                            *heading_level,
                        )
                        .unwrap(),
                        *item_style,
                    )
                    .unwrap()
                })
                .collect(),
        )
        .unwrap()
    }

    fn render(raw: &str) -> String {
        super::render_for_creation(raw, &preset("default"))
    }

    fn is_placeholder_body(raw: &str) -> bool {
        super::is_placeholder_body(&TaskBody::new(raw))
    }

    fn append_marker_sections(body: &str, raw: &str) -> String {
        super::append_marker_sections(body, raw, &preset("default"))
    }

    #[test]
    fn placeholder_body_detection() {
        assert!(is_placeholder_body(""));
        assert!(is_placeholder_body("TODO define this"));
        assert!(is_placeholder_body("definir prompt"));
        assert!(!is_placeholder_body("add startup toggle"));
    }

    #[test]
    fn content_renders_marker_first_body_without_body_leakage() {
        assert_eq!(
            render("/c context"),
            format!("## Goals\n{S}## Context{S}- context")
        );
    }

    #[test]
    fn content_wraps_a_normal_body() {
        assert_eq!(render("add startup toggle"), "## Goals\n");
        assert_eq!(render("a / b"), format!("## Goals{S}- b"));
    }

    #[test]
    fn content_renders_one_bullet_per_slash_section() {
        assert_eq!(
            render("create engine feature to add update task / make it idempotent"),
            format!("## Goals{S}- make it idempotent")
        );
    }

    #[test]
    fn content_preserves_ampersands_as_text() {
        assert_eq!(render("a & b"), "## Goals\n");
    }

    #[test]
    fn content_keeps_placeholder_raw_so_it_stays_detectable() {
        assert_eq!(render("TODO"), "TODO");
        assert!(is_placeholder_body(&render("TODO")));
        assert!(is_placeholder_body(&render("tbd")));
        assert!(is_placeholder_body(&render("define prompt")));
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
            assert!(is_placeholder_body(raw), "should be placeholder: {raw:?}");
        }
        for raw in [
            "add startup toggle",
            "todolist cleanup",
            "a / b",
            "/c context",
        ] {
            assert!(
                !is_placeholder_body(raw),
                "should not be placeholder: {raw:?}"
            );
        }
    }

    #[test]
    fn unicode_todo_suffix_remains_raw_without_becoming_placeholder() {
        for body in ["TODOé", "[!] TODOé"] {
            assert!(!is_placeholder_body(body));
            assert_eq!(render(body), body);
        }
        assert!(is_placeholder_body("TODO-implement"));
    }

    #[test]
    fn append_marker_sections_grows_an_existing_section_in_place() {
        assert_eq!(
            append_marker_sections("## Goals\n- do the thing\n", "also this"),
            "## Goals\n- do the thing\n- also this\n"
        );
    }

    #[test]
    fn append_marker_sections_creates_a_missing_section_at_the_end() {
        assert_eq!(
            append_marker_sections("## Goals\n- do the thing\n", "another goal /c new context"),
            "## Goals\n- do the thing\n- another goal\n\n## Context\n\n- new context\n"
        );
    }

    #[test]
    fn append_marker_sections_marker_first_leaves_goals_untouched() {
        assert_eq!(
            append_marker_sections("## Goals\n- do the thing\n", "/c context"),
            "## Goals\n- do the thing\n\n## Context\n\n- context\n"
        );
    }

    #[test]
    fn append_preserves_markdown_examples_inside_sections() {
        // Indented lines after a list item continue that item, so the list stays open.
        for (example, goal_separator) in [
            ("```markdown\n## Context\nexample\n```", "\n\n"),
            ("~~~\n## Context\nexample\n~~~", "\n\n"),
            ("    ## Context\n    example", "\n"),
            ("> ## Context\n> example", "\n\n"),
        ] {
            let body = format!("## Goals\n\n- keep\n\n{example}\n\n## Context\n\n- old");
            assert_eq!(
                append_marker_sections(&body, "new goal /c new context"),
                format!(
                    "## Goals\n\n- keep\n\n{example}{goal_separator}- new goal\n\n## Context\n\n- old\n- new context\n"
                )
            );
        }
    }

    #[test]
    fn built_in_alt_preset_renders_level_one_goals_and_paragraph_context() {
        assert_eq!(
            super::render_for_creation(
                "title / first goal / second goal /c first context / second context",
                &preset("alt")
            ),
            format!(
                "# Goals{S}- first goal\n- second goal{S}## Context{S}first context{S}second context"
            )
        );
    }

    #[test]
    fn append_finds_sections_at_any_heading_level() {
        assert_eq!(
            append_marker_sections("#### Goals\n\n- one\n\n# Context\n\n- old", "two /c new"),
            "#### Goals\n\n- one\n- two\n\n# Context\n\n- old\n- new\n"
        );
    }

    #[test]
    fn append_continues_the_existing_item_style() {
        let preset = custom_preset(&[
            ("/g", "Goals", 3, MarkerSectionItemStyle::Numbered),
            ("/c", "Context", 3, MarkerSectionItemStyle::Paragraph),
            ("/n", "Notes", 3, MarkerSectionItemStyle::Bullet),
        ]);
        let body =
            "### Goals\n\n1. one\n2. two\n\n### Context\n\n- legacy bullet\n\n### Notes\n\nprose";
        assert_eq!(
            super::append_marker_sections(body, "three / four /c added /n more", &preset),
            "### Goals\n\n1. one\n2. two\n3. three\n4. four\n\n### Context\n\n- legacy bullet\n- added\n\n### Notes\n\nprose\n\nmore\n"
        );
        assert_eq!(
            super::append_marker_sections("### Goals\n\n7) seven", "eight", &preset),
            "### Goals\n\n7) seven\n8) eight\n"
        );
        assert_eq!(
            super::append_marker_sections("### Goals\n\n* star", "next", &preset),
            "### Goals\n\n* star\n* next\n"
        );
    }

    #[test]
    fn append_uses_the_configured_style_for_empty_and_missing_sections() {
        let preset = custom_preset(&[
            ("/g", "Goals", 3, MarkerSectionItemStyle::Numbered),
            ("/c", "Context", 4, MarkerSectionItemStyle::Paragraph),
        ]);
        assert_eq!(
            super::append_marker_sections("### Goals\n", "one / two /c first / second", &preset),
            "### Goals\n\n1. one\n2. two\n\n#### Context\n\nfirst\n\nsecond\n"
        );
    }

    #[test]
    fn append_report_collapses_multiline_into_a_single_line_block() {
        assert_eq!(
            append_report("body\n", "line one line two"),
            "body\n\n### Report\n\nline one line two\n"
        );
    }
}
