use regex::Regex;

/// Flips a done link (`- [x] [[ID]] ...`) back to an open link (`- [ ] [[ID]]`),
/// preserving indentation. Returns `None` when no done link for `id` exists.
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
