use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Style, Stylize as _},
    text::{Line, Span},
    widgets::{Block, BorderType, Clear, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::{
    app::{App, Dialog, ProjectPurpose},
    browser::{Record, SearchMode},
    draft::{DraftKind, FieldKind, TaskAction},
};

const COLOR_ACCENT: Color = Color::Rgb(0xe8, 0x73, 0x0c);
const COLOR_ACCENT_TEXT: Color = Color::Rgb(0x1f, 0x24, 0x30);
const COLOR_ACTIVE: Color = Color::Rgb(0xff, 0xcc, 0x66);
const COLOR_LABEL: Color = Color::Rgb(0xdf, 0xbf, 0xff);
const COLOR_FOCUSED_ROW: Color = Color::Rgb(0x2d, 0x33, 0x47);
const COLOR_BADGE: Color = Color::Rgb(0xf2, 0x87, 0x79);
const COLOR_STATUS: Color = Color::Rgb(0x95, 0xe6, 0xcb);
const COLOR_MUTED: Color = Color::Rgb(0x70, 0x7a, 0x8c);

const HELP: &[&str] = &[
    "BROWSE",
    "↑/↓ or j/k   Move between records",
    "Enter        Open task actions; open note in editor",
    "/            Search names; Enter keeps the query, Esc leaves search",
    "f / Ctrl-g   Toggle names and saved contents (Ctrl-g while searching)",
    "p · s · Tab  Choose project · cycle status · cycle tasks/notes",
    "PgUp/PgDn    Scroll the Markdown preview",
    "r / F5       Refresh saved contents and reconnect",
    "t · n        Create task from template · create note",
    "e · m        Edit Markdown in editor · edit task metadata",
    "d · D · c    Complete quickly · complete with report · cancel with report",
    "b · a · Del  Move to backlog · activate/reopen · delete",
    "Space · u    Mark task references · clear marked references",
    "y · x        Copy [[ID]] references · export to a new file",
    "q / Ctrl-c   Quit; running writes finish first",
    "",
    "DRAFTS",
    "Tab/Shift-Tab Change field",
    "Ctrl-s        Save through pwf-server",
    "Ctrl-e        Edit the focused field with VISUAL, EDITOR, or vi",
    "←/→           Choose priority/effort; Delete selects none",
    "Enter         Newline in body; open selector in blocker field",
    "F5            Inspect saved state after a failed write; keep your input",
    "Esc           Return; unsaved changes ask before discarding",
    "",
    "BLOCKERS AND CONFIRMATIONS",
    "Type to search all projects; ↑/↓ move; Space marks; Enter applies",
    "Ctrl-u clears blockers; Esc keeps the previous blocker field",
    "Confirmations default to No. ←/→ or y/n choose; Enter answers; Esc cancels",
    "Server confirmations expire after 25 seconds; ↑/↓ scroll their details",
];

pub(super) fn render(frame: &mut Frame, app: &mut App) {
    let [main, feedback, status, help] = Layout::vertical([
        Constraint::Min(6),
        Constraint::Length(4),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(frame.area());
    if main.width >= 80 {
        let [left, right] =
            Layout::horizontal([Constraint::Percentage(45), Constraint::Percentage(55)])
                .areas(main);
        let [stack, records] =
            Layout::vertical([Constraint::Length(8), Constraint::Min(3)]).areas(left);
        render_stack(frame, stack, app);
        render_records(frame, records, app);
        render_preview(frame, right, app);
    } else {
        let [stack, records, preview] = Layout::vertical([
            Constraint::Length(7),
            Constraint::Percentage(40),
            Constraint::Min(3),
        ])
        .areas(main);
        render_stack(frame, stack, app);
        render_records(frame, records, app);
        render_preview(frame, preview, app);
    }
    frame.render_widget(
        Paragraph::new(clean(&app.notice.text))
            .style(Style::new().fg(if app.notice.is_error() {
                COLOR_BADGE
            } else {
                COLOR_STATUS
            }))
            .wrap(Wrap { trim: false })
            .block(panel(" Feedback ")),
        feedback,
    );
    let status_text = app.pending.as_ref().map_or_else(
        || {
            format!(
                "{} visible / {} loaded  ·  {} marked references",
                app.browser.visible.len(),
                app.browser.records.len(),
                app.browser.references.len()
            )
        },
        |pending| {
            format!(
                "{} {}",
                ["◐", "◓", "◑", "◒"][app.tick % 4],
                if pending.writes {
                    "Saving… Wait for the result before quitting."
                } else {
                    "Loading…  Esc cancels this read"
                }
            )
        },
    );
    frame.render_widget(Paragraph::new(status_text.fg(COLOR_STATUS)), status);
    frame.render_widget(Paragraph::new(footer_keys(app).fg(COLOR_ACCENT)), help);
    if app.draft.is_some() {
        render_draft(frame, main, app);
    }
    if let Some(dialog) = &mut app.dialog {
        render_dialog(frame, main, dialog, &app.browser.projects);
    }
    if app.confirmation.is_some() {
        render_confirmation(frame, main, app);
    }
}

fn footer_keys(app: &App) -> &'static str {
    if app.confirmation.is_some() {
        return "←→ choose  enter answer  esc cancel";
    }
    match &app.dialog {
        Some(Dialog::Blockers(_)) => "type search  ↑↓ move  space mark  enter apply  esc return",
        Some(Dialog::Inspection { .. }) => {
            "↑↓ or PgUp/PgDn scroll  enter accept saved state  esc return"
        }
        Some(Dialog::Help { .. }) => "↑↓ or PgUp/PgDn scroll  enter or esc return",
        Some(Dialog::Projects { .. } | Dialog::Actions { .. }) => {
            "↑↓ move  enter select  esc return"
        }
        None if app.draft.is_some() => {
            "tab field  ctrl-s save  ctrl-e editor  F5 inspect  esc return"
        }
        None => "↑↓ move  / search  enter actions  t task  n note  space mark  ? keys  q quit",
    }
}

fn render_stack(frame: &mut Frame, area: Rect, app: &mut App) {
    let block = panel(" PWF · Stack ");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let [fields, search] =
        Layout::vertical([Constraint::Length(4), Constraint::Length(1)]).areas(inner);
    let rows = [
        (
            "project",
            app.browser
                .project
                .as_ref()
                .map_or_else(|| "all active projects".into(), ToString::to_string),
        ),
        ("status", app.browser.status.label().to_string()),
        ("records", app.browser.kind.label().to_string()),
        ("search", app.browser.search_mode.label().to_string()),
    ];
    let lines = rows
        .into_iter()
        .map(|(label, value)| {
            Line::from(vec![
                format!("{label:8} ").fg(COLOR_LABEL),
                value.fg(COLOR_ACTIVE),
            ])
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines), fields);
    if app.browser.searching {
        app.browser.query.set_cursor_line_style(Style::default());
        app.browser
            .query
            .set_cursor_style(Style::new().fg(COLOR_ACCENT_TEXT).bg(COLOR_ACCENT));
        app.browser
            .query
            .set_placeholder_text("type to search; Ctrl-g toggles saved contents");
        frame.render_widget(&app.browser.query, search);
    } else {
        let query = app.browser.query.lines().join(" ");
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                "/        ".fg(COLOR_ACCENT),
                clean(&query).fg(COLOR_MUTED),
            ])),
            search,
        );
    }
}

fn render_records(frame: &mut Frame, area: Rect, app: &App) {
    let query = app.browser.query.lines().join(" ");
    let start = app
        .browser
        .selected
        .saturating_sub(usize::from(area.height) / 2);
    let rows = app
        .browser
        .visible
        .iter()
        .skip(start)
        .take(usize::from(area.height))
        .map(|index| {
            let record = &app.browser.records[*index];
            let marked = record
                .id
                .task()
                .is_some_and(|id| app.browser.references.contains(id));
            record_row(record, &query, app.browser.search_mode, marked)
        })
        .collect::<Vec<_>>();
    if rows.is_empty() {
        frame.render_widget(
            Paragraph::new("No matching records.\nChange the search, status, or project filter.")
                .fg(COLOR_MUTED)
                .wrap(Wrap { trim: true })
                .block(panel(" Tasks + notes ")),
            area,
        );
    } else {
        render_list(
            frame,
            area,
            " Tasks + notes ".into(),
            rows,
            app.browser.selected - start,
        );
    }
}

fn record_row(record: &Record, query: &str, mode: SearchMode, marked: bool) -> ListItem<'static> {
    let line = Line::from(vec![
        (if marked { "[x] " } else { "    " }).fg(if marked { COLOR_ACTIVE } else { COLOR_MUTED }),
        format!("{} ", record.id.as_str()).fg(record_color(record)),
        format!("[{}] ", record.status_label()).fg(COLOR_MUTED),
        Span::raw(clean(&record.title)),
    ]);
    if mode != SearchMode::Contents || query.is_empty() {
        return ListItem::new(line);
    }
    let Some((number, text)) = record.content_match(query) else {
        return ListItem::new(line);
    };
    ListItem::new(vec![
        line,
        Line::from(format!(
            "    {}  {}",
            number + 1,
            clean(&text.chars().take(240).collect::<String>())
        ))
        .fg(COLOR_MUTED),
    ])
}

fn render_preview(frame: &mut Frame, area: Rect, app: &App) {
    let block = panel("").title(Line::from(vec![
        " Markdown ".fg(COLOR_ACCENT).bold(),
        " live preview "
            .fg(COLOR_ACCENT_TEXT)
            .bg(COLOR_BADGE)
            .bold(),
    ]));
    let inner = block.inner(area);
    let Some(record) = app.browser.current() else {
        frame.render_widget(block, area);
        return;
    };
    let header = [
        Line::from(vec![
            format!("{}  ", record.id.as_str())
                .fg(record_color(record))
                .bold(),
            clean(&record.title).bold(),
        ]),
        Line::from(clean(&record.path.display().to_string())).fg(COLOR_MUTED),
        Line::from(""),
    ];
    let metadata = record.metadata.iter().map(|(label, value)| {
        Line::from(vec![
            format!("{label:11} ").fg(COLOR_LABEL),
            Span::raw(clean(value)),
        ])
    });
    let diagnostic = record
        .diagnostic
        .as_ref()
        .map(|text| Line::from(clean(text)).fg(COLOR_BADGE));
    let body = record.body.lines().map(|line| {
        let text = clean(
            &line
                .chars()
                .take(usize::from(inner.width) * 4)
                .collect::<String>(),
        );
        if line.starts_with('#') {
            Line::from(text).fg(COLOR_ACTIVE).bold()
        } else if line.starts_with("- ") || line.starts_with("* ") {
            Line::from(text).fg(COLOR_STATUS)
        } else {
            Line::from(text)
        }
    });
    let lines = header
        .into_iter()
        .chain(metadata)
        .chain(diagnostic)
        .chain([Line::from("")])
        .chain(body)
        .skip(app.browser.preview_scroll)
        .take(usize::from(inner.height))
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(block),
        area,
    );
}

fn render_draft(frame: &mut Frame, bounds: Rect, app: &mut App) {
    let Some(draft) = &mut app.draft else {
        return;
    };
    let area = popup(bounds, 86, 86);
    frame.render_widget(Clear, area);
    let block = panel(format!(" {} ", draft.title()));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let [fields_area, advice_area] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(4)]).areas(inner);
    let constraints = draft
        .fields
        .iter()
        .map(|field| {
            if field.kind == FieldKind::Body {
                Constraint::Min(4)
            } else {
                Constraint::Length(3)
            }
        })
        .collect::<Vec<_>>();
    let areas = Layout::vertical(constraints).split(fields_area);
    for (index, (field, area)) in draft.fields.iter_mut().zip(areas.iter()).enumerate() {
        let focused = index == draft.focused;
        field.input.set_block(
            panel(format!(" {} ", field.label)).border_style(Style::new().fg(if focused {
                COLOR_ACCENT
            } else {
                COLOR_MUTED
            })),
        );
        field
            .input
            .set_cursor_line_style(Style::new().bg(if focused {
                COLOR_FOCUSED_ROW
            } else {
                Color::Reset
            }));
        field.input.set_cursor_style(if focused {
            Style::new().fg(COLOR_ACCENT_TEXT).bg(COLOR_ACCENT)
        } else {
            Style::default()
        });
        frame.render_widget(&field.input, *area);
    }
    let advice = if draft.requires_inspection {
        "Write failed. Your input is retained. F5 inspects current saved state before Ctrl-s can retry."
    } else {
        match &draft.kind {
            DraftKind::Metadata(_) => {
                "Only changed fields are saved. Empty tags/blockers and 'none' tiers clear their values. Enter in Blocked by selects tasks across projects."
            }
            DraftKind::Export { references } => references,
            _ => {
                "Tab changes fields · Ctrl-e opens the focused field in your editor · Ctrl-s saves · Esc returns."
            }
        }
    };
    let advice = if app.notice.is_error() {
        app.notice.text.as_str()
    } else {
        advice
    };
    frame.render_widget(
        Paragraph::new(clean(advice))
            .fg(if app.notice.is_error() {
                COLOR_BADGE
            } else {
                COLOR_MUTED
            })
            .wrap(Wrap { trim: false }),
        advice_area,
    );
}

fn render_dialog(
    frame: &mut Frame,
    bounds: Rect,
    dialog: &mut Dialog,
    projects: &[crate::browser::Project],
) {
    let area = popup(bounds, 76, 80);
    frame.render_widget(Clear, area);
    match dialog {
        Dialog::Help { scroll } => {
            *scroll = (*scroll).min(HELP.len().saturating_sub(1));
            render_scrolled_text(
                frame,
                area,
                " Keyboard · ↑↓ scroll · Enter/Esc returns ",
                &HELP.join("\n"),
                *scroll,
            );
        }
        Dialog::Inspection {
            current, scroll, ..
        } => render_scrolled_text(
            frame,
            area,
            " Saved state · ↑↓ scroll · Enter accepts · Esc returns ",
            current,
            *scroll,
        ),
        Dialog::Actions { id, selected } => {
            let rows = TaskAction::ALL
                .iter()
                .map(|action| ListItem::new(action.label()))
                .collect();
            render_list(frame, area, format!(" Actions · {id} "), rows, *selected);
        }
        Dialog::Projects { purpose, selected } => {
            let mut rows = Vec::new();
            if matches!(purpose, ProjectPurpose::Browse) {
                rows.push(ListItem::new("all active projects"));
            }
            rows.extend(projects.iter().map(|project| {
                ListItem::new(format!("{}  {}", project.id, clean(&project.title)))
            }));
            render_list(
                frame,
                area,
                " Project · Enter selects · Esc returns ".into(),
                rows,
                *selected,
            );
        }
        Dialog::Blockers(picker) => {
            let [search, list, selected] = Layout::vertical([
                Constraint::Length(3),
                Constraint::Min(2),
                Constraint::Length(3),
            ])
            .areas(area);
            picker.query.set_block(panel(" Search all projects "));
            picker.query.set_cursor_line_style(Style::default());
            frame.render_widget(&picker.query, search);
            let rows = picker
                .visible()
                .iter()
                .map(|index| {
                    let candidate = &picker.candidates[*index];
                    ListItem::new(format!(
                        "{} {} [{}] {}",
                        if picker.marked.contains(&candidate.id) {
                            "[x]"
                        } else {
                            "[ ]"
                        },
                        candidate.id,
                        candidate.status,
                        clean(&candidate.title)
                    ))
                })
                .collect();
            render_list(
                frame,
                list,
                " Blocked by · Space marks · Enter applies ".into(),
                rows,
                picker.selected,
            );
            frame.render_widget(
                Paragraph::new(
                    picker
                        .marked
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(", "),
                )
                .wrap(Wrap { trim: true })
                .fg(COLOR_ACTIVE)
                .block(panel(" Selected · Ctrl-u clears ")),
                selected,
            );
        }
    }
}

fn render_scrolled_text(frame: &mut Frame, area: Rect, title: &str, text: &str, scroll: usize) {
    let lines = text
        .lines()
        .skip(scroll)
        .take(usize::from(area.height))
        .map(|line| {
            clean(
                &line
                    .chars()
                    .take(usize::from(area.width) * 4)
                    .collect::<String>(),
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(panel(title)),
        area,
    );
}

fn render_confirmation(frame: &mut Frame, bounds: Rect, app: &App) {
    let Some(confirmation) = &app.confirmation else {
        return;
    };
    let area = popup(bounds, 76, 65);
    frame.render_widget(Clear, area);
    let block = panel(format!(" {} ", clean(&confirmation.question.title)))
        .border_style(Style::new().fg(COLOR_BADGE));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let [details, buttons] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(2)]).areas(inner);
    frame.render_widget(
        Paragraph::new(
            confirmation
                .question
                .lines
                .iter()
                .map(|line| clean(line))
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .wrap(Wrap { trim: false })
        .scroll((confirmation.scroll, 0)),
        details,
    );
    let choices = [false, true].map(|affirmative| {
        let label = if affirmative { " Yes " } else { " No · keep " };
        if affirmative == confirmation.affirmative {
            label.fg(COLOR_ACCENT_TEXT).bg(COLOR_ACCENT).bold()
        } else {
            label.fg(COLOR_MUTED)
        }
    });
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            choices[0].clone(),
            Span::raw("  "),
            choices[1].clone(),
            "  Enter answers · Esc cancels".fg(COLOR_MUTED),
        ])),
        buttons,
    );
}

fn render_list(
    frame: &mut Frame,
    area: Rect,
    title: String,
    rows: Vec<ListItem<'_>>,
    selected: usize,
) {
    frame.render_stateful_widget(
        List::new(rows)
            .block(panel(title))
            .highlight_style(Style::new().fg(COLOR_ACCENT_TEXT).bg(COLOR_ACCENT).bold()),
        area,
        &mut ListState::default().with_selected(Some(selected)),
    );
}

fn record_color(record: &Record) -> Color {
    match record.status_label() {
        "done" | "verified note" => Color::Rgb(0xa3, 0xe6, 0x35),
        "cancelled" => COLOR_BADGE,
        "backlog" => COLOR_ACTIVE,
        _ => Color::Rgb(0x64, 0x95, 0xed),
    }
}

fn panel(title: impl Into<String>) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(COLOR_MUTED))
        .title(title.into().fg(COLOR_ACCENT).bold())
}

fn popup(area: Rect, width: u16, height: u16) -> Rect {
    let [_, center, _] = Layout::vertical([
        Constraint::Percentage((100 - height) / 2),
        Constraint::Percentage(height),
        Constraint::Min(0),
    ])
    .areas(area);
    let [_, center, _] = Layout::horizontal([
        Constraint::Percentage((100 - width) / 2),
        Constraint::Percentage(width),
        Constraint::Min(0),
    ])
    .areas(center);
    center
}

fn clean(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend};

    use super::*;
    use crate::test_support::app;

    #[test]
    fn baseline_panels_and_focus_render_at_wide_and_narrow_sizes() {
        for (width, height) in [(120, 36), (60, 30), (24, 10)] {
            let mut app = app();
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|frame| render(frame, &mut app)).unwrap();
            if width < 60 {
                continue;
            }
            let screen = terminal
                .backend()
                .buffer()
                .content()
                .chunks(width as usize)
                .map(|row| {
                    row.iter()
                        .map(ratatui::buffer::Cell::symbol)
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
                .join("\n");
            assert!(
                ["PWF · Stack", "Tasks + notes", "live preview", "PWF-0007"]
                    .iter()
                    .all(|text| screen.contains(text)),
                "{screen}"
            );
            assert!(
                terminal
                    .backend()
                    .buffer()
                    .content()
                    .iter()
                    .any(|cell| cell.bg == COLOR_ACCENT)
            );
        }
    }
}
