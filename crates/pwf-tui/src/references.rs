use std::{io::Write as _, path::Path};

use anyhow::{Context as _, Result, ensure};
use base64::{Engine as _, engine::general_purpose::STANDARD};

pub(super) fn copy(text: &str) -> Result<()> {
    ensure!(
        text.len() <= 64 * 1024,
        "References exceed the terminal clipboard's 64 KiB limit; export them instead."
    );
    let mut output = std::io::stdout().lock();
    write!(output, "\x1b]52;c;{}\x07", STANDARD.encode(text))?;
    output.flush()?;
    Ok(())
}

pub(super) fn export(path: &Path, references: &str) -> Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("Cannot create an export in {}", parent.display()))?;
    writeln!(file, "{references}")?;
    file.as_file().sync_all()?;
    file.persist_noclobber(path).with_context(|| {
        format!(
            "Cannot export to {}; choose a new file path",
            path.display()
        )
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exports_complete_wikilinks_without_overwriting_existing_text() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("references.md");
        export(&path, "[[PWF-0007]] [[AUX-0003]]").unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "[[PWF-0007]] [[AUX-0003]]\n"
        );
        assert!(export(&path, "[[PWF-0008]]").is_err());
        assert_eq!(
            std::fs::read_to_string(path).unwrap(),
            "[[PWF-0007]] [[AUX-0003]]\n"
        );
    }
}
