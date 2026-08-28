use pwf_models::task::TaskId;

use super::{
    markdown_line,
    task_link::{self, Checkbox},
};

/// Flips a done link (`- [x] [[ID]] ...`) back to an open link (`- [ ] [[ID]]`),
/// preserving indentation. Returns `None` when no done link for `id` exists.
pub(super) fn reopen_done_link(content: &str, id: &TaskId) -> Option<String> {
    let (line, parsed_link) = markdown_line::lines(content).find_map(|line| {
        task_link::parse(line.text)
            .filter(|link| link.checkbox == Some(Checkbox::Done) && link.id == id.as_ref())
            .map(|link| (line, link))
    })?;
    Some(format!(
        "{}{}- [ ] [[{id}]]{}{}",
        &content[..line.start],
        parsed_link.indentation,
        line.newline,
        &content[line.end..]
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reopens_only_the_requested_done_link() {
        let id = TaskId::try_new("FOO-0001").unwrap();
        let content = "  - [x] [[FOO-0001|done task]] \u{2705} 2026-08-04\n- [x] [[FOO-0002]]\n";

        let updated = reopen_done_link(content, &id);

        assert_eq!(
            updated.as_deref(),
            Some("  - [ ] [[FOO-0001]]\n- [x] [[FOO-0002]]\n")
        );
    }
}
