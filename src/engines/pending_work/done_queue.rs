// Rotating done-queue for the project index (PWF-0026): `check` keeps a capped,
// per-section queue of `- [x] [[ID]] ✅ date` entries at the bottom of each
// section instead of deleting the link, evicting the oldest past the cap. Pure
// string transforms — the caller does the item-file stamping and archiving.

use regex::Regex;

// ! Per-section retention caps. "General" is the default region before the first
// ! `## ` header. Easy-to-read source-of-truth table; a ≤4-entry linear scan with
// ! no allocation — strictly cheaper than an embedded/parsed config.
const SECTION_CAPS: &[(&str, usize)] =
    &[("General", 6), ("Low-prio", 3), ("Future", 3), ("Human", 3)];

/// Retention cap for a canonical section name, or `None` (retain without eviction)
/// for sections outside the table.
fn section_cap(section: &str) -> Option<usize> {
    SECTION_CAPS
        .iter()
        .find(|(name, _)| *name == section)
        .map(|(_, cap)| *cap)
}

/// Canonical section name for a `## ` header label (case-insensitive). `Futuro`
/// folds to `Future`. Unknown labels are returned lowercased and match no cap.
fn canonical_section(label: &str) -> String {
    match label.trim().to_lowercase().as_str() {
        "future" | "futuro" => "Future".to_string(),
        "human" => "Human".to_string(),
        "low-prio" | "low-priority" => "Low-prio".to_string(),
        other => other.to_string(),
    }
}

/// Outcome of marking an item done in the index.
pub struct DoneQueue {
    /// Rewritten index content.
    pub content: String,
    /// Ids evicted past their section cap, oldest-first (caller archives them).
    pub evicted: Vec<String>,
    /// True if a `## Futuro` header was normalized to `## Future`.
    pub futuro_renamed: bool,
}

/// Rewrite `id`'s open link to `- [x] [[id]] ✅ date` at the bottom of its
/// section, then evict the oldest done entries beyond the section cap. Also
/// normalizes any `## Futuro` header to `## Future`.
pub fn mark_done(content: &str, id: &str, date: &str) -> DoneQueue {
    let header_re = Regex::new(r"^##\s+(?P<label>.+?)\s*$").unwrap();
    let futuro_re = Regex::new(r"(?i)^##\s+futuro\s*$").unwrap();
    // Open/bare link for this id (not a `[x]` done line); tolerates a legacy
    // `|title` alias.
    let open_re = Regex::new(&format!(
        r"^\s*-\s*(?:\[ \]\s*)?\[\[{}(?:\|[^\]]*)?\]\]",
        regex::escape(id)
    ))
    .unwrap();
    // Any done link, capturing its id (for eviction/archival); alias-tolerant.
    let done_re =
        Regex::new(r"^\s*-\s*\[[xX]\]\s*\[\[(?P<id>[A-Z]{2,4}-\d{4})(?:\|[^\]]*)?\]\]").unwrap();

    let date_re = Regex::new(r"✅\s*(\d{4}-\d{2}-\d{2})").unwrap();

    let had_trailing_nl = content.ends_with('\n');
    let mut lines: Vec<String> = content.split('\n').map(str::to_string).collect();
    if had_trailing_nl {
        lines.pop(); // drop the empty element split() appends after a trailing \n
    }

    // Normalize `## Futuro` → `## Future` before anything else.
    let mut futuro_renamed = false;
    for line in &mut lines {
        if futuro_re.is_match(line) {
            *line = "## Future".to_string();
            futuro_renamed = true;
        }
    }

    // Locate the item's open line; mark it done in place (layout untouched).
    let Some(target) = lines.iter().position(|l| open_re.is_match(l)) else {
        // Not open in the index (already done / missing) — nothing to rotate.
        return DoneQueue {
            content: join(&lines, had_trailing_nl),
            evicted: Vec::new(),
            futuro_renamed,
        };
    };
    lines[target] = format!("- [x] [[{id}]] ✅ {date}");
    let section = section_at_line(&lines, target, &header_re);

    // Evict the oldest done entries in the touched section beyond its cap.
    let mut evicted = Vec::new();
    if let Some(cap) = section_cap(&section) {
        // (line index, completion date) for each done entry in this section.
        let mut done: Vec<(usize, String)> = lines
            .iter()
            .enumerate()
            .filter(|(i, l)| {
                done_re.is_match(l) && section_at_line(&lines, *i, &header_re) == section
            })
            .map(|(i, l)| {
                let d = date_re
                    .captures(l)
                    .map(|c| c[1].to_string())
                    .unwrap_or_default();
                (i, d)
            })
            .collect();
        if done.len() > cap {
            // Oldest first by date; line index is the deterministic tiebreak.
            done.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
            let mut victims: Vec<usize> = done
                .iter()
                .take(done.len() - cap)
                .map(|(i, _)| *i)
                .collect();
            for &i in &victims {
                if let Some(c) = done_re.captures(&lines[i]) {
                    evicted.push(c["id"].to_string());
                }
            }
            // Remove highest-index-first so earlier indices stay valid.
            victims.sort_unstable_by(|a, b| b.cmp(a));
            for i in victims {
                lines.remove(i);
            }
        }
    }

    DoneQueue {
        content: join(&lines, had_trailing_nl),
        evicted,
        futuro_renamed,
    }
}

/// Canonical section name governing line `idx` (the nearest `## ` header above
/// it), or `General` for the pre-header region.
fn section_at_line(lines: &[String], idx: usize, header_re: &Regex) -> String {
    lines[..idx]
        .iter()
        .rev()
        .find_map(|l| {
            header_re
                .captures(l)
                .map(|c| canonical_section(&c["label"]))
        })
        .unwrap_or_else(|| "General".to_string())
}

fn join(lines: &[String], trailing_nl: bool) -> String {
    let mut s = lines.join("\n");
    if trailing_nl {
        s.push('\n');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_marks_item_done_in_place_under_cap() {
        let content = "- [ ] [[PWF-0002]]\n- [ ] [[PWF-0001]]\n";
        let out = mark_done(content, "PWF-0001", "2026-06-13");
        assert_eq!(
            out.content,
            "- [ ] [[PWF-0002]]\n- [x] [[PWF-0001]] ✅ 2026-06-13\n"
        );
        assert!(out.evicted.is_empty());
        assert!(!out.futuro_renamed);
    }

    fn done(id: &str, day: u32) -> String {
        format!("- [x] [[{id}]] ✅ 2026-01-{day:02}")
    }

    #[test]
    fn general_section_evicts_oldest_beyond_cap_of_six() {
        // Six existing done + check a seventh → oldest (topmost) evicted.
        let mut body: Vec<String> = (1..=6).map(|n| done(&format!("PWF-{n:04}"), n)).collect();
        body.push("- [ ] [[PWF-0007]]".to_string());
        let content = format!("{}\n", body.join("\n"));
        let out = mark_done(&content, "PWF-0007", "2026-06-13");
        assert_eq!(out.evicted, vec!["PWF-0001"]);
        assert!(!out.content.contains("PWF-0001"));
        assert!(out.content.contains("- [x] [[PWF-0007]] ✅ 2026-06-13"));
        // Still six done after eviction.
        assert_eq!(out.content.matches("- [x]").count(), 6);
    }

    #[test]
    fn future_section_cap_is_three_and_general_untouched() {
        let content = format!(
            "- [ ] [[PWF-0010]]\n\n## Future\n{}\n{}\n{}\n- [ ] [[PWF-0004]]\n",
            done("PWF-0001", 1),
            done("PWF-0002", 2),
            done("PWF-0003", 3),
        );
        let out = mark_done(&content, "PWF-0004", "2026-06-13");
        assert_eq!(out.evicted, vec!["PWF-0001"]);
        assert!(out.content.contains("- [ ] [[PWF-0010]]"), "general kept");
        assert!(out.content.contains("- [x] [[PWF-0004]] ✅ 2026-06-13"));
    }

    #[test]
    fn futuro_header_is_normalized_to_future() {
        let content = "## Futuro\n- [ ] [[PWF-0001]]\n";
        let out = mark_done(content, "PWF-0001", "2026-06-13");
        assert!(out.futuro_renamed);
        assert!(out.content.contains("## Future"));
        assert!(!out.content.contains("Futuro"));
        assert!(out.content.contains("- [x] [[PWF-0001]] ✅ 2026-06-13"));
    }

    #[test]
    fn section_header_match_is_case_insensitive() {
        // `## low-prio` (lowercase) must still resolve to the Low-prio cap of 3.
        let content = format!(
            "## low-prio\n{}\n{}\n{}\n- [ ] [[PWF-0004]]\n",
            done("PWF-0001", 1),
            done("PWF-0002", 2),
            done("PWF-0003", 3),
        );
        let out = mark_done(&content, "PWF-0004", "2026-06-13");
        assert_eq!(out.evicted, vec!["PWF-0001"]);
    }

    #[test]
    fn aliased_open_link_is_matched_and_rewritten_bare() {
        // Legacy `[[ID|title]]` form must still check, normalizing to the bare
        // done line (titles render via CSS now).
        let content = "- [ ] [[GLP-0001|tray gui]]\n";
        let out = mark_done(content, "GLP-0001", "2026-06-13");
        assert_eq!(out.content, "- [x] [[GLP-0001]] ✅ 2026-06-13\n");
    }

    #[test]
    fn missing_or_already_done_id_leaves_content_unchanged() {
        let content = "- [x] [[PWF-0001]] ✅ 2026-01-01\n";
        let out = mark_done(content, "PWF-0099", "2026-06-13");
        assert_eq!(out.content, content);
        assert!(out.evicted.is_empty());
    }
}
