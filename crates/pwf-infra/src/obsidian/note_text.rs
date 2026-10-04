use pwf_application::ports::task_vault::TaskBodyWrite;

use super::MarkdownFile;

/// Replaces the body while preserving frontmatter.
///
/// A note without a closing fence is treated as body-only.
pub(super) fn replace_body(file: &MarkdownFile, body: &TaskBodyWrite) -> String {
    let source = file.source();
    let prefix = &source[..source.len() - file.body().len()];
    let mut updated = prefix.to_string();
    if !prefix.is_empty() {
        if !prefix.ends_with('\n') {
            updated.push('\n');
        }
        if matches!(body, TaskBodyWrite::Rendered(_)) {
            updated.push('\n');
        }
    }
    match body {
        TaskBodyWrite::Rendered(body) => {
            updated.push_str(body.trim_end());
            updated.push('\n');
        }
        TaskBodyWrite::Verbatim(body) => updated.push_str(body.as_ref()),
    }
    updated
}

#[cfg(test)]
mod tests {
    use pwf_models::task::TaskBody;

    use super::*;

    #[test]
    fn body_replacement_preserves_frontmatter() {
        let source = "---\nid: FOO-0001\nstatus: active\ntitle: old\n---\n\nold body\n";

        let file = MarkdownFile::from_source("task.md", source.to_string());
        assert_eq!(
            replace_body(&file, &TaskBodyWrite::Rendered("new body".into())),
            "---\nid: FOO-0001\nstatus: active\ntitle: old\n---\n\nnew body\n"
        );
    }

    #[test]
    fn body_only_source_stays_body_only() {
        let file = MarkdownFile::from_source("task.md", "old body\n".into());
        assert_eq!(
            replace_body(&file, &TaskBodyWrite::Rendered("new body\n\n".into())),
            "new body\n"
        );
    }

    #[test]
    fn verbatim_replacement_preserves_body_bytes_and_frontmatter_formatting() {
        for prefix in [
            "---\nid: FOO-0001\nstatus: active\n---\n",
            "\u{feff}--- \r\n# keep this comment\r\nstatus: active\r\n---\t\r\n",
        ] {
            let file = MarkdownFile::from_source("task.md", format!("{prefix}\nold body\n"));
            for body in ["", "---\n# Authored /g\r\n\r\n  \r\n", "no final newline"] {
                assert_eq!(
                    replace_body(&file, &TaskBodyWrite::Verbatim(TaskBody::new(body))),
                    format!("{prefix}{body}")
                );
            }
        }
    }
}
