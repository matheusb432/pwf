use prompt_lanes::parse;
use pwf_models::pending_work::{TaskTitle, TaskTitleError};

pub(super) fn inferred(prompt: &str) -> Result<TaskTitle, TaskTitleError> {
    TaskTitle::try_new(parse(prompt).title)
}
