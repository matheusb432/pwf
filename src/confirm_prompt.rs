//! Domain-agnostic confirmation-prompt text: a titled, frontmatter-style
//! metadata block plus a closing question. Kept free of any engine's domain
//! types so every [`crate::confirm::Confirm`] caller (session dispatch, task
//! removal, …) renders its last-look prompt the same way — and readably on a
//! narrow terminal, as stacked `key: value` lines rather than a wide Markdown
//! table.

use std::fmt;

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

/// A last-look confirmation prompt: a `# title`, a frontmatter-style `key: value`
/// block, then a closing `question`. [`fmt::Display`] renders it to the prompt
/// text handed to [`crate::confirm::Confirm::confirm`], which appends the
/// `[Y/n]` hint; `question` supplies its own trailing `?`.
///
/// A borrowing view — like [`std::fmt::Arguments`] or [`std::path::Display`] —
/// it holds a `&[Field]` into the caller's stack array rather than owning a
/// `Vec`, so building a prompt allocates nothing.
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
    fn no_fields_still_renders_header_and_question() {
        let prompt = ConfirmationPrompt::new("Confirm", &[], "Proceed?");
        assert_eq!(prompt.to_string(), "# Confirm\n\n\nProceed?");
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
