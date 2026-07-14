use pwf_domain::pending_work::AddedItem;

use super::add::{AddPendingWorkError, AddPendingWorkItem};
use crate::ports::{CompleteItemSpec, PendingWorkWriteStore, StatusTransitionOutput};

#[derive(Debug, Clone)]
pub struct CompletePendingWork {
    pub id: String,
    pub completed: String,
    pub report: Option<String>,
    pub commits: Vec<String>,
    pub review: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum CompletePendingWorkError {
    #[error("{0}")]
    WriteStore(Box<dyn std::error::Error + Send + Sync>),
    #[error("{0}")]
    ReviewTask(#[source] AddPendingWorkError),
}

#[cqrsy::handler(command)]
pub fn execute(
    command: CompletePendingWork,
    store: &impl PendingWorkWriteStore,
) -> Result<StatusTransitionOutput, CompletePendingWorkError> {
    let commits = frontmatter_value(&command.commits);
    let closed = store
        .complete_item(CompleteItemSpec {
            id: command.id.clone(),
            completed: command.completed.clone(),
            report: command.report,
            commits: commits.clone(),
        })
        .map_err(|error| CompletePendingWorkError::WriteStore(Box::new(error)))?;
    let mut text = closed.to_output_text();
    let review_item = if command.review {
        let review = super::add::execute(
            AddPendingWorkItem {
                project_name: closed.project.clone(),
                prompt: review_task_prompt(&closed.id, commits.as_deref()),
                title: None,
                created: command.completed,
                section: Some("Human".to_string()),
                prereq: None,
                effort: None,
                tags: None,
            },
            store,
        )
        .map_err(CompletePendingWorkError::ReviewTask)?;
        text.push_str(&added_item_raw_text(&review));
        Some(review)
    } else {
        None
    };
    Ok(StatusTransitionOutput {
        text,
        diagnostics: closed.diagnostics,
        review_item,
    })
}

pub(super) fn frontmatter_value(values: &[String]) -> Option<String> {
    let mut ranges: Vec<String> = Vec::new();
    for value in values {
        for raw in value.split(',') {
            let raw = raw.trim();
            if raw.is_empty() {
                continue;
            }
            if !ranges.iter().any(|range| range == raw) {
                ranges.push(raw.to_string());
            }
        }
    }
    (!ranges.is_empty()).then(|| ranges.join(", "))
}

pub(super) fn review_task_prompt(reviewed_id: &str, range: Option<&str>) -> String {
    let (title, diff) = match range {
        Some(range) => (
            format!("review {reviewed_id}, commits: {range}"),
            format!("git-tools diff {range}"),
        ),
        None => (
            format!("review {reviewed_id}"),
            "git-tools diff".to_string(),
        ),
    };
    format!("{title} / {diff} / git-tools diff-subrepos")
}

pub(super) fn added_item_raw_text(item: &AddedItem) -> String {
    format!(
        "ADDED PWF TASK [{}] {} :: {}\n  file: {}\n",
        item.id,
        item.project,
        item.title,
        item.note_path.display()
    )
}

#[cfg(test)]
mod tests {
    use super::{CompletePendingWorkError, frontmatter_value, review_task_prompt};
    use crate::pending_work::add::AddPendingWorkError;

    fn values(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn commit_ranges_trim_split_and_deduplicate_in_first_seen_order() {
        assert_eq!(
            frontmatter_value(&values(&[" a..b,c..d ", "a..b", " e..f "])),
            Some("a..b, c..d, e..f".to_string())
        );
        assert_eq!(frontmatter_value(&values(&["", "  ", ","])), None);
    }

    #[test]
    fn review_prompt_uses_scoped_or_bare_diff() {
        assert_eq!(
            review_task_prompt("PWF-0128", Some("a..b")),
            "review PWF-0128, commits: a..b / git-tools diff a..b / git-tools diff-subrepos"
        );
        assert_eq!(
            review_task_prompt("PWF-0128", None),
            "review PWF-0128 / git-tools diff / git-tools diff-subrepos"
        );
    }

    #[test]
    fn review_task_error_retains_the_concrete_add_error() {
        let error = CompletePendingWorkError::ReviewTask(AddPendingWorkError::WriteStore(
            Box::new(std::io::Error::other("index write failed")),
        ));

        let CompletePendingWorkError::ReviewTask(source) = error else {
            panic!("expected review-task error");
        };
        assert!(matches!(source, AddPendingWorkError::WriteStore(_)));
    }
}
