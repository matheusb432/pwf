use std::{
    fs, io,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

fn temp_suffix(now: SystemTime) -> String {
    match now.duration_since(UNIX_EPOCH) {
        Ok(duration) => format!("tmp-{}", duration.as_nanos()),
        Err(error) => format!("tmp-before-{}", error.duration().as_nanos()),
    }
}

/// Write `content` to `path` atomically (temp sibling + rename). UTF-8 (no BOM);
/// callers must pass `\n`-delimited content. Never leaves a `.bak` — notes-pro is
/// git-tracked, so prior content is recoverable from git.
pub fn write_text_atomic(path: &Path, content: &str) -> io::Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
        && !parent.exists()
    {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(temp_suffix(SystemTime::now()));
    fs::write(&tmp, content.as_bytes())?;
    fs::rename(&tmp, path)?; // same-volume atomic replace
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_file_and_never_leaves_a_bak() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.md");
        fs::write(&p, "old content\n").unwrap();
        write_text_atomic(&p, "new\n").unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "new\n");
        assert!(!dir.path().join("a.md.bak").exists());
    }

    #[test]
    fn temp_suffix_handles_pre_epoch_clock() {
        let before_epoch = std::time::UNIX_EPOCH - std::time::Duration::from_nanos(7);

        assert_eq!(temp_suffix(before_epoch), "tmp-before-7");
    }
}
