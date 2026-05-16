use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub struct Parsed {
    pub frontmatter: BTreeMap<String, String>,
    pub body: String,
}

fn is_fm_key(key: &str) -> bool {
    let mut chars = key.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Parse YAML-ish frontmatter exactly like `ConvertFrom-Frontmatter`.
/// BOM- and CRLF-tolerant. Non-matching input yields empty frontmatter and the
/// whole text as the body.
pub fn parse(text: &str) -> Parsed {
    let stripped = text.strip_prefix('\u{feff}').unwrap_or(text);
    let lines: Vec<&str> = stripped.split('\n').collect();
    let opens = !lines.is_empty() && lines[0].trim_end_matches('\r') == "---";
    if !opens {
        return Parsed {
            frontmatter: BTreeMap::new(),
            body: text.to_string(),
        };
    }
    let close = (1..lines.len()).find(|&i| lines[i].trim_end_matches('\r') == "---");
    let Some(close) = close else {
        return Parsed {
            frontmatter: BTreeMap::new(),
            body: text.to_string(),
        };
    };

    let mut frontmatter = BTreeMap::new();
    for line in &lines[1..close] {
        let l = line.trim_end_matches('\r');
        if let Some(idx) = l.find(':') {
            let key = &l[..idx];
            if is_fm_key(key) {
                let val = &l[idx + 1..];
                let val = val.strip_prefix(' ').unwrap_or(val); // strip one optional leading space
                frontmatter.insert(key.to_string(), val.trim().to_string());
            }
        }
    }
    let body = lines[close + 1..].join("\n");
    Parsed { frontmatter, body }
}

/// Return `text` with frontmatter lines whose key is in `drop_keys` removed.
/// Body, key order, and per-line formatting are preserved. Text without a
/// frontmatter block is returned unchanged. BOM-/CRLF-tolerant like [`parse`].
pub fn strip_frontmatter_keys(text: &str, drop_keys: &[&str]) -> String {
    let stripped = text.strip_prefix('\u{feff}').unwrap_or(text);
    let lines: Vec<&str> = stripped.split('\n').collect();
    let opens = !lines.is_empty() && lines[0].trim_end_matches('\r') == "---";
    if !opens {
        return text.to_string();
    }
    let Some(close) = (1..lines.len()).find(|&i| lines[i].trim_end_matches('\r') == "---") else {
        return text.to_string();
    };
    let mut out: Vec<&str> = Vec::with_capacity(lines.len());
    out.push(lines[0]); // opening ---
    for line in &lines[1..close] {
        let l = line.trim_end_matches('\r');
        if let Some(idx) = l.find(':') {
            let key = &l[..idx];
            if is_fm_key(key) && drop_keys.contains(&key) {
                continue;
            }
        }
        out.push(line);
    }
    out.extend_from_slice(&lines[close..]); // closing --- + body
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fm(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn parses_frontmatter_and_body() {
        let p = parse("---\nstatus: active\ntitle: tray gui\n---\n\nadd startup toggle\n");
        assert_eq!(
            p.frontmatter,
            fm(&[("status", "active"), ("title", "tray gui")])
        );
        assert_eq!(p.body.trim(), "add startup toggle");
    }

    #[test]
    fn tolerates_bom_and_crlf() {
        let p = parse("\u{feff}---\r\nstatus: done\r\n---\r\nbody\r\n");
        assert_eq!(p.frontmatter.get("status").unwrap(), "done");
        assert_eq!(p.body.trim(), "body");
    }

    #[test]
    fn no_frontmatter_returns_whole_text_as_body() {
        let p = parse("# title\n\n- [[GLP-0001|x]]\n");
        assert!(p.frontmatter.is_empty());
        assert_eq!(p.body, "# title\n\n- [[GLP-0001|x]]\n");
    }

    #[test]
    fn strip_drops_listed_keys_preserving_order() {
        let note =
            "---\nstatus: active\ntitle: t\nproject: pwf\ncreated: 2026-06-20\n---\n\nbody line\n";
        let out = strip_frontmatter_keys(note, &["created"]);
        assert_eq!(
            out,
            "---\nstatus: active\ntitle: t\nproject: pwf\n---\n\nbody line\n"
        );
    }

    #[test]
    fn strip_keeps_keys_not_in_denylist() {
        let note = "---\nstatus: active\nprereq: \"[[PWF-0001]]\"\n---\n\nbody\n";
        let out = strip_frontmatter_keys(note, &["created"]);
        assert_eq!(out, note);
    }

    #[test]
    fn strip_passes_through_text_without_frontmatter() {
        let note = "Goals:\n- do a thing\n";
        assert_eq!(strip_frontmatter_keys(note, &["created"]), note);
    }

    #[test]
    fn strip_is_bom_and_crlf_tolerant() {
        let note = "\u{feff}---\r\nstatus: active\r\ncreated: 2026-06-20\r\n---\r\nbody\r\n";
        let out = strip_frontmatter_keys(note, &["created"]);
        assert!(!out.contains("created"));
        assert!(out.contains("status: active"));
        assert!(out.contains("body"));
    }
}
