use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Parsed {
    pub frontmatter: BTreeMap<String, String>,
    pub body: String,
}

fn is_frontmatter_key(key: &str) -> bool {
    let mut characters = key.chars();
    match characters.next() {
        Some(character) if character.is_ascii_alphabetic() => {}
        _ => return false,
    }
    characters
        .all(|character| character.is_ascii_alphanumeric() || character == '_' || character == '-')
}

pub(super) fn parse(text: &str) -> Parsed {
    let stripped = text.strip_prefix('\u{feff}').unwrap_or(text);
    let lines: Vec<&str> = stripped.split('\n').collect();
    let opens = !lines.is_empty() && lines[0].trim_end_matches('\r') == "---";
    if !opens {
        return Parsed {
            frontmatter: BTreeMap::new(),
            body: text.to_string(),
        };
    }
    let close = (1..lines.len()).find(|&index| lines[index].trim_end_matches('\r') == "---");
    let Some(close) = close else {
        return Parsed {
            frontmatter: BTreeMap::new(),
            body: text.to_string(),
        };
    };

    let mut frontmatter = BTreeMap::new();
    for line in &lines[1..close] {
        let line = line.trim_end_matches('\r');
        if let Some(index) = line.find(':') {
            let key = &line[..index];
            if is_frontmatter_key(key) {
                let value = &line[index + 1..];
                let value = value.strip_prefix(' ').unwrap_or(value);
                frontmatter.insert(key.to_string(), value.trim().to_string());
            }
        }
    }
    let body = lines[close + 1..].join("\n");
    Parsed { frontmatter, body }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::parse;

    fn frontmatter(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect()
    }

    #[test]
    fn parses_frontmatter_and_body() {
        let parsed = parse("---\nstatus: active\ntitle: tray gui\n---\n\nadd startup toggle\n");
        assert_eq!(
            parsed.frontmatter,
            frontmatter(&[("status", "active"), ("title", "tray gui")])
        );
        assert_eq!(parsed.body.trim(), "add startup toggle");
    }

    #[test]
    fn tolerates_bom_and_crlf() {
        let parsed = parse("\u{feff}---\r\nstatus: done\r\n---\r\nbody\r\n");
        assert_eq!(parsed.frontmatter.get("status").unwrap(), "done");
        assert_eq!(parsed.body.trim(), "body");
    }

    #[test]
    fn missing_or_malformed_frontmatter_returns_the_original_body() {
        for source in [
            "# title\n\n- [[GLP-0001|x]]\n",
            "---\nstatus: active\nunterminated\n",
        ] {
            let parsed = parse(source);
            assert!(parsed.frontmatter.is_empty());
            assert_eq!(parsed.body, source);
        }
    }
}
