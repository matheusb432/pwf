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

pub(super) fn write_text_atomic(path: &Path, content: &str) -> io::Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
        && !parent.exists()
    {
        fs::create_dir_all(parent)?;
    }
    let temporary_path = path.with_extension(temp_suffix(SystemTime::now()));
    fs::write(&temporary_path, content.as_bytes())?;
    fs::rename(&temporary_path, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        time::{Duration, UNIX_EPOCH},
    };

    use super::{temp_suffix, write_text_atomic};

    #[test]
    fn writes_file_and_never_leaves_a_backup() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("a.md");
        fs::write(&path, "old content\n").unwrap();

        write_text_atomic(&path, "new\n").unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "new\n");
        assert!(!directory.path().join("a.md.bak").exists());
    }

    #[test]
    fn temporary_suffix_handles_pre_epoch_clock() {
        assert_eq!(
            temp_suffix(UNIX_EPOCH - Duration::from_nanos(7)),
            "tmp-before-7"
        );
    }
}
