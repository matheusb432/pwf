//! Formats domain-agnostic confirmation prompts as a title, metadata lines, and a question.

use std::fmt;

/// Stores one `key: value` metadata line.
pub struct Field {
    key: &'static str,
    value: String,
}

impl Field {
    /// Creates a metadata line and flattens newlines in `value`.
    pub fn new(key: &'static str, value: impl Into<String>) -> Self {
        Field {
            key,
            value: flatten(value.into()),
        }
    }
}

/// Borrows and formats a Markdown title, metadata block, and closing question.
/// The terminal shell appends the confirmation hint after this rendered text.
pub struct ConfirmationPrompt<'a> {
    title: &'a str,
    fields: &'a [Field],
    question: &'a str,
}

impl<'a> ConfirmationPrompt<'a> {
    pub fn new(title: &'a str, fields: &'a [Field], question: &'a str) -> Self {
        ConfirmationPrompt {
            title,
            fields,
            question,
        }
    }
}

impl fmt::Display for ConfirmationPrompt<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "# {}\n", self.title)?;
        for field in self.fields {
            writeln!(f, "{}: {}", field.key, field.value)?;
        }
        write!(f, "\n{}", self.question)
    }
}

/// Collapses line endings so each value stays on one metadata line.
fn flatten(value: String) -> String {
    if value.contains(['\r', '\n']) {
        // Replace CRLF first to avoid two spaces for one Windows line ending.
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
        let fields = [
            Field::new("task_id", "PWF-0001"),
            Field::new("inline", "true"),
            Field::new("agent", "claude"),
        ];
        let prompt = ConfirmationPrompt::new("Confirm removal", &fields, "Remove this task?");

        assert_eq!(
            prompt.to_string(),
            "# Confirm removal\n\ntask_id: PWF-0001\ninline: true\nagent: claude\n\nRemove this task?"
        );
    }

    #[test]
    fn multiline_values_are_flattened_to_one_line() {
        let fields = [Field::new("title", "first\nsecond\r\nthird")];
        let prompt = ConfirmationPrompt::new("Confirm", &fields, "Go?");
        assert!(
            prompt.to_string().contains("title: first second third\n"),
            "value must stay on one line: {prompt}"
        );
    }
}
