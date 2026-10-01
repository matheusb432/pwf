use std::io::Write as _;

use anyhow::Result;
use tempfile::NamedTempFile;

pub(super) fn create(text: &str) -> Result<NamedTempFile> {
    let mut file = tempfile::Builder::new()
        .prefix("pwf-draft-")
        .suffix(".md")
        .tempfile()?;
    file.write_all(text.as_bytes())?;
    file.flush()?;
    Ok(file)
}
