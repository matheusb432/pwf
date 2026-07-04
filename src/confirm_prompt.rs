//! Domain-agnostic confirmation-prompt text: a titled, frontmatter-style
//! metadata block plus a closing question. Kept free of any engine's domain
//! types so every [`crate::confirm::Confirm`] caller (session dispatch, task
//! removal, …) renders its last-look prompt the same way — and readably on a
//! narrow terminal, as stacked `key: value` lines rather than a wide Markdown
//! table.

/// One `key: value` line of a confirmation's metadata block.
pub struct Field {
    key: &'static str,
    value: String,
}

impl Field {
    /// A metadata line. `value` is flattened to a single line so the block
    /// stays scannable even when a value (e.g. a title) carries newlines.
    pub fn new(key: &'static str, value: impl Into<String>) -> Self {
        Field {
            key,
            value: flatten(value.into()),
        }
    }
}

/// Render `# {title}`, the frontmatter-style `key: value` block, then
/// `{question}` — the prompt text handed to [`crate::confirm::Confirm::confirm`],
/// which appends the `[Y/n]` hint. `question` supplies its own trailing `?`.
pub fn confirmation_prompt(title: &str, fields: &[Field], question: &str) -> String {
    let mut out = format!("# {title}\n\n");
    for field in fields {
        out.push_str(field.key);
        out.push_str(": ");
        out.push_str(&field.value);
        out.push('\n');
    }
    out.push('\n');
    out.push_str(question);
    out
}

/// Collapse any carriage return / newline to a space so a value stays on its
/// own `key: value` line.
fn flatten(value: String) -> String {
    if value.contains(['\r', '\n']) {
        // Collapse CRLF to one space before flattening stray lone CR/LF, so a
        // Windows line ending doesn't leave a double gap.
        value.replace("\r\n", " ").replace(['\r', '\n'], " ")
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_header_metadata_block_and_question() {
        let out = confirmation_prompt(
            "Confirm removal",
            &[
                Field::new("task_id", "PWF-0001"),
                Field::new("inline", "true"),
                Field::new("agent", "claude"),
            ],
            "Remove this task?",
        );

        assert_eq!(
            out,
            "# Confirm removal\n\ntask_id: PWF-0001\ninline: true\nagent: claude\n\nRemove this task?"
        );
    }

    #[test]
    fn no_fields_still_renders_header_and_question() {
        let out = confirmation_prompt("Confirm", &[], "Proceed?");
        assert_eq!(out, "# Confirm\n\n\nProceed?");
    }

    #[test]
    fn multiline_values_are_flattened_to_one_line() {
        let out = confirmation_prompt(
            "Confirm",
            &[Field::new("title", "first\nsecond\r\nthird")],
            "Go?",
        );
        assert!(
            out.contains("title: first second third\n"),
            "value must stay on one line: {out}"
        );
    }
}
