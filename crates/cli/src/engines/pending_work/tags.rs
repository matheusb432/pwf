use pwf_domain::pending_work::{ParseTagsError, Tags};

use super::errors::PendingWorkError;

pub(super) fn from_flags(values: &[String]) -> Result<Option<Tags>, PendingWorkError> {
    if values.is_empty() {
        return Ok(None);
    }
    Tags::parse_values(values)
        .map(Some)
        .map_err(|e| map_error(&e))
}

fn map_error(error: &ParseTagsError) -> PendingWorkError {
    PendingWorkError::InvalidTag {
        raw: error.raw().to_string(),
    }
}
