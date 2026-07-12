use std::sync::LazyLock;

use regex::Regex;

static HEADER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^##\s+(?P<label>.+?)\s*$").unwrap());
static FUTURO_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^##\s+futuro\s*$").unwrap());
static DONE_LINK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*-\s*\[[xX]\]\s*\[\[(?P<id>[A-Z]{2,4}-\d{4})(?:\|[^\]]*)?\]\]").unwrap()
});
static DATE_STAMP_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"✅\s*(\d{4}-\d{2}-\d{2})").unwrap());

const SECTION_CAPS: &[(&str, usize)] =
    &[("General", 6), ("Low-prio", 3), ("Future", 3), ("Human", 3)];

fn section_cap(section: &str) -> Option<usize> {
    SECTION_CAPS
        .iter()
        .find(|(name, _)| *name == section)
        .map(|(_, cap)| *cap)
}

fn canonical_section(label: &str) -> String {
    match label.trim().to_lowercase().as_str() {
        "future" | "futuro" => "Future".to_string(),
        "human" => "Human".to_string(),
        "low-prio" | "low-priority" => "Low-prio".to_string(),
        other => other.to_string(),
    }
}

pub(super) struct DoneQueue {
    pub content: String,
    pub evicted: Vec<String>,
    pub futuro_renamed: bool,
}
pub(super) fn mark_done(content: &str, id: &str, date: &str) -> DoneQueue {
    let open_re = Regex::new(&format!(
        r"^\s*-\s*(?:\[ \]\s*)?\[\[{}(?:\|[^\]]*)?\]\]",
        regex::escape(id)
    ))
    .unwrap();

    let had_trailing_nl = content.ends_with('\n');
    let mut lines: Vec<String> = content.split('\n').map(str::to_string).collect();
    if had_trailing_nl {
        lines.pop();
    }

    let mut futuro_renamed = false;
    for line in &mut lines {
        if FUTURO_RE.is_match(line) {
            *line = "## Future".to_string();
            futuro_renamed = true;
        }
    }

    let Some(target) = lines.iter().position(|line| open_re.is_match(line)) else {
        return DoneQueue {
            content: join(&lines, had_trailing_nl),
            evicted: Vec::new(),
            futuro_renamed,
        };
    };
    lines[target] = format!("- [x] [[{id}]] ✅ {date}");
    let section = section_at_line(&lines, target, &HEADER_RE);

    // TODO: refactor: this function (mark_done) has the side effect of evicting. this should instead be handled via an event.
    // the data flow would be in application -> CompleteItem checks it. dispatches CompleteItemEvent -> will handle the eviction done queue.
    let mut evicted = Vec::new();
    if let Some(cap) = section_cap(&section) {
        let mut done: Vec<(usize, String, String)> = lines
            .iter()
            .enumerate()
            .filter(|(index, line)| {
                DONE_LINK_RE.is_match(line)
                    && section_at_line(&lines, *index, &HEADER_RE) == section
            })
            .map(|(index, line)| {
                let completed = DATE_STAMP_RE
                    .captures(line)
                    .map(|captures| captures[1].to_string())
                    .unwrap_or_default();
                let task_id = DONE_LINK_RE
                    .captures(line)
                    .map(|captures| captures["id"].to_string())
                    .unwrap_or_default();
                (index, completed, task_id)
            })
            .collect();
        if done.len() > cap {
            done.sort_by(|left, right| left.1.cmp(&right.1).then(left.2.cmp(&right.2)));
            let mut victims: Vec<usize> = done
                .iter()
                .take(done.len() - cap)
                .map(|(index, _, _)| *index)
                .collect();
            for &index in &victims {
                if let Some(captures) = DONE_LINK_RE.captures(&lines[index]) {
                    evicted.push(captures["id"].to_string());
                }
            }
            victims.sort_unstable_by(|left, right| right.cmp(left));
            for index in victims {
                lines.remove(index);
            }
        }
    }

    DoneQueue {
        content: join(&lines, had_trailing_nl),
        evicted,
        futuro_renamed,
    }
}

pub(super) fn reopen_done_link(content: &str, id: &str) -> Option<String> {
    let done_re = Regex::new(&format!(
        r"(?m)^(?P<indent>\s*)-\s*\[[xX]\]\s*\[\[{}(?:\|[^\]]*)?\]\].*$",
        regex::escape(id)
    ))
    .unwrap();
    if !done_re.is_match(content) {
        return None;
    }
    let open_line = format!("${{indent}}- [ ] [[{id}]]");
    Some(done_re.replace(content, open_line.as_str()).into_owned())
}

fn section_at_line(lines: &[String], index: usize, header_re: &Regex) -> String {
    lines[..index]
        .iter()
        .rev()
        .find_map(|line| {
            header_re
                .captures(line)
                .map(|captures| canonical_section(&captures["label"]))
        })
        .unwrap_or_else(|| "General".to_string())
}

fn join(lines: &[String], trailing_nl: bool) -> String {
    let mut text = lines.join("\n");
    if trailing_nl {
        text.push('\n');
    }
    text
}
