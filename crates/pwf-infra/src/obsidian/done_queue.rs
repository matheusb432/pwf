use pwf_models::task::TaskId;
use regex::Regex;

/// Flips a done link (`- [x] [[ID]] ...`) back to an open link (`- [ ] [[ID]]`),
/// preserving indentation. Returns `None` when no done link for `id` exists.
pub(super) fn reopen_done_link(content: &str, id: &TaskId) -> Option<String> {
    let done_re = Regex::new(&format!(
        r"(?m)^(?P<indent>\s*)-\s*\[[xX]\]\s*\[\[{}(?:\|[^\]]*)?\]\].*$",
        regex::escape(id.as_ref())
    ))
    .ok()?;
    if !done_re.is_match(content) {
        return None;
    }
    let open_line = format!("${{indent}}- [ ] [[{id}]]");
    Some(done_re.replace(content, open_line.as_str()).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reopens_only_the_requested_done_link() {
        let id = match TaskId::try_new("PWF-0001") {
            Ok(id) => id,
            Err(error) => panic!("test task ID must be valid: {error}"),
        };
        let content = "  - [x] [[PWF-0001|done task]] \u{2705} 2026-08-04\n- [x] [[PWF-0002]]\n";

        let updated = reopen_done_link(content, &id);

        assert_eq!(
            updated.as_deref(),
            Some("  - [ ] [[PWF-0001]]\n- [x] [[PWF-0002]]\n")
        );
    }
}
