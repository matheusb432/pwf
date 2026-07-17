use std::{
    fmt::Write,
    path::{Path, PathBuf},
};

use crate::{cli::EngineArgs, config, fs_atomic::write_text_atomic};

#[derive(Debug, thiserror::Error)]
pub enum MigrateError {
    #[error("Missing --config-path")]
    MissingConfigPath,
    #[error("{0}")]
    Config(
        #[from]
        #[source]
        crate::config::ConfigError,
    ),
    #[error("Failed to read {}: {source}", path.display())]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("Failed to create {}: {source}", path.display())]
    Create {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("Failed to write {}: {source}", path.display())]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("Failed to delete {}: {source}", path.display())]
    Delete {
        path: PathBuf,
        source: std::io::Error,
    },
}

struct ParsedNote {
    items: Vec<AgentItem>,
    /// Human-authored lines retained in the project index.
    index_content: String,
}

struct AgentItem {
    title: String,
    status: String, // "active" | "done"
    body: String,
    completed: Option<String>,
}

/// Extracts checkbox tasks and an immediately following text fence while preserving other lines.
fn convert_to_agent_items(content: &str) -> ParsedNote {
    let content_lf = content.replace("\r\n", "\n");
    let lines: Vec<&str> = content_lf.split('\n').collect();
    let mut items: Vec<AgentItem> = Vec::new();
    let mut output: Vec<&str> = Vec::new();

    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if let Some((mark, title, rest)) = parse_checkbox(line) {
            let mut body_lines: Vec<String> = Vec::new();
            let rest_stripped = strip_leading_marker(&rest);
            if !rest_stripped.is_empty() {
                body_lines.push(rest_stripped);
            }
            if i + 1 < lines.len() && is_fence_open(lines[i + 1]) {
                i += 2;
                while i < lines.len() && !is_fence_close(lines[i]) {
                    body_lines.push(lines[i].to_string());
                    i += 1;
                }
                // Leave `i` on the closing fence, or at `lines.len()` when unterminated.
            }
            let body = body_lines.join("\n").trim().to_string();
            let completed = extract_date(&body);
            let status = if mark == ' ' {
                "active".to_string()
            } else {
                "done".to_string()
            };
            items.push(AgentItem {
                title,
                status,
                body,
                completed,
            });
        } else {
            output.push(line);
        }
        i += 1;
    }

    let index_content = format!(
        "{}\n",
        output
            .join("\n")
            .trim_end_matches('\n')
            .trim_end_matches('\r')
    );
    ParsedNote {
        items,
        index_content,
    }
}

/// Parses a checkbox task into its mark, title, and trailing text.
fn parse_checkbox(line: &str) -> Option<(char, String, String)> {
    let s = line.trim_start();
    if !s.starts_with('-') {
        return None;
    }
    let after_dash = &s[1..];
    let after_ws = after_dash.trim_start_matches([' ', '\t']);
    if after_ws.len() == after_dash.len() {
        return None;
    }
    if !after_ws.starts_with('[') {
        return None;
    }
    let after_bracket = &after_ws[1..];
    if after_bracket.is_empty() {
        return None;
    }
    let mark = after_bracket.chars().next().unwrap();
    if mark != ' ' && mark != 'x' && mark != 'X' {
        return None;
    }
    let after_mark = &after_bracket[1..];
    if !after_mark.starts_with(']') {
        return None;
    }
    let after_close = &after_mark[1..];
    let after_ws2 = after_close.trim_start_matches([' ', '\t']);
    if after_ws2.len() == after_close.len() {
        return None;
    }
    if !after_ws2.starts_with('`') {
        return None;
    }
    let after_bt = &after_ws2[1..];
    let close_bt = after_bt.find('`')?;
    let title = &after_bt[..close_bt];
    let rest = &after_bt[close_bt + 1..];
    Some((mark, title.to_string(), rest.to_string()))
}

/// Removes a leading `<-+` or `::` marker.
fn strip_leading_marker(s: &str) -> String {
    let t = s.trim_start();
    if t.starts_with('<') {
        let mut idx = 1usize;
        while idx < t.len() && t.as_bytes()[idx] == b'-' {
            idx += 1;
        }
        if idx > 1 {
            return t[idx..].trim_start().to_string();
        }
    }
    if let Some(after_colons) = t.strip_prefix("::") {
        return after_colons.trim_start().to_string();
    }
    t.to_string()
}

/// Reports whether `line` opens a bare or `text`-tagged code fence.
fn is_fence_open(line: &str) -> bool {
    let t = line.trim();
    t == "```text" || t == "```"
}

/// Reports whether `line` is a bare closing code fence.
fn is_fence_close(line: &str) -> bool {
    line.trim() == "```"
}

/// Extracts the first YYYY-MM-DD date from text.
fn extract_date(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i + 10 <= bytes.len() {
        if bytes[i..i + 4].iter().all(u8::is_ascii_digit)
            && bytes[i + 4] == b'-'
            && bytes[i + 5..i + 7].iter().all(u8::is_ascii_digit)
            && bytes[i + 7] == b'-'
            && bytes[i + 8..i + 10].iter().all(u8::is_ascii_digit)
        {
            return Some(std::str::from_utf8(&bytes[i..i + 10]).unwrap().to_string());
        }
        i += 1;
    }
    None
}

/// Renders one item with `completed` as the final frontmatter field when present.
fn new_item_content(
    title: &str,
    status: &str,
    project: &str,
    created: &str,
    completed: Option<&str>,
    body: &str,
) -> String {
    let mut out = String::new();
    out.push_str("---\n");
    let _ = writeln!(out, "status: {status}");
    let _ = writeln!(out, "title: {title}");
    let _ = writeln!(out, "project: {project}");
    let _ = writeln!(out, "created: {created}");
    if let Some(c) = completed {
        let _ = writeln!(out, "completed: {c}");
    }
    out.push_str("---\n\n");
    if !body.is_empty() {
        out.push_str(body);
        out.push('\n');
    }
    out
}

/// Migrates flat project notes and returns string errors for CLI dispatch.
pub fn run(args: &EngineArgs) -> Result<String, String> {
    run_typed(args).map_err(|e| e.to_string())
}

/// Migrates flat project notes into folder indexes and item files.
pub fn run_typed(args: &EngineArgs) -> Result<String, MigrateError> {
    let config_path = args
        .config_path
        .clone()
        .or_else(config::default_config_path)
        .ok_or(MigrateError::MissingConfigPath)?;
    let cfg = config::load(&config_path, args.notes_dir.as_deref())?;
    let notes_root = Path::new(&cfg.notes_dir);
    let created_default = args
        .date
        .clone()
        .unwrap_or_else(|| chrono::Local::now().format("%Y-%m-%d").to_string());

    for name in cfg.prefixes.keys() {
        let prefix = &cfg.prefixes[name];
        let flat_note = notes_root.join(format!("{name}.md"));
        let folder = notes_root.join(name);
        let index_path = folder.join(format!("{name}.md"));

        if index_path.exists() {
            continue;
        }
        if !flat_note.exists() {
            continue;
        }

        let content = std::fs::read_to_string(&flat_note).map_err(|source| MigrateError::Read {
            path: flat_note.clone(),
            source,
        })?;
        let parsed = convert_to_agent_items(&content);

        let mut ordered: Vec<AgentItem> = parsed.items;
        ordered.reverse();

        let mut active_links: Vec<String> = Vec::new();
        let ids: Vec<String> = (0..ordered.len())
            .map(|i| format!("{prefix}-{:04}", i + 1))
            .collect();
        for (i, item) in ordered.iter().enumerate() {
            if item.status == "active" {
                active_links.push(format!("- [ ] [[{}]]", ids[i]));
            }
        }

        let index_base = parsed
            .index_content
            .trim_end_matches('\n')
            .trim_end_matches('\r');
        let index_content = if active_links.is_empty() {
            format!("{index_base}\n")
        } else {
            let links_block = active_links.join("\n");
            if index_base.is_empty() {
                format!("{links_block}\n")
            } else {
                format!("{links_block}\n\n{index_base}\n")
            }
        };

        if args.dry_run {
            eprintln!("Would migrate {}", flat_note.display());
            continue;
        }

        std::fs::create_dir_all(&folder).map_err(|source| MigrateError::Create {
            path: folder.clone(),
            source,
        })?;

        for (i, item) in ordered.iter().enumerate() {
            let id = &ids[i];
            let item_path = folder.join(format!("{id}.md"));
            let item_created = item.completed.as_deref().unwrap_or(&created_default);
            let content = new_item_content(
                &item.title,
                &item.status,
                name,
                item_created,
                item.completed.as_deref(),
                &item.body,
            );
            write_text_atomic(&item_path, &content).map_err(|source| MigrateError::Write {
                path: item_path,
                source,
            })?;
        }

        write_text_atomic(&index_path, &index_content).map_err(|source| MigrateError::Write {
            path: index_path.clone(),
            source,
        })?;

        std::fs::remove_file(&flat_note).map_err(|source| MigrateError::Delete {
            path: flat_note.clone(),
            source,
        })?;
    }

    Ok(String::new())
}
