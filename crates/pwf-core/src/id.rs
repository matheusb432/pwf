//! Sequential id allocation by scanning a project dir for `{KEY}-NNNN.md` files.

use std::path::Path;

use regex::Regex;

/// Allocate `"{key}-{max+1:04}"` by scanning every dir in `dirs` for `*.md`
/// files whose stem matches `^{key}-(\d{4})$`. Missing dirs are skipped; gaps
/// are preserved (allocation is `max + 1`, never a gap-fill).
///
/// # Panics
/// Panics if the internal id-matching regex fails to compile — unreachable in
/// practice since `key` is escaped via [`regex::escape`].
pub fn next_id(dirs: &[&Path], key: &str) -> String {
    let re = Regex::new(&format!(r"^{}-(\d{{4}})$", regex::escape(key))).unwrap();
    let mut max = 0;
    for dir in dirs {
        scan_max(dir, &re, &mut max);
    }
    format!("{key}-{:04}", max + 1)
}

fn scan_max(dir: &Path, re: &Regex, max: &mut i32) {
    if !dir.exists() {
        return;
    }
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = entry.path();
        if p.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        if let Some(stem) = p.file_stem().and_then(|s| s.to_str())
            && let Some(c) = re.captures(stem)
        {
            let n: i32 = c[1].parse().unwrap_or(0);
            if n > *max {
                *max = n;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn allocates_one_when_empty() {
        let d = tempdir();
        assert_eq!(next_id(&[d.path()], "PWF"), "PWF-0001");
    }

    #[test]
    fn allocates_max_plus_one_across_dirs() {
        let d = tempdir();
        let arch = d.path().join("_archive");
        std::fs::create_dir_all(&arch).unwrap();
        std::fs::write(d.path().join("PWF-0002.md"), "x").unwrap();
        std::fs::write(arch.join("PWF-0005.md"), "x").unwrap();
        assert_eq!(next_id(&[d.path(), arch.as_path()], "PWF"), "PWF-0006");
    }

    #[test]
    fn disjoint_keys_do_not_perturb_each_other() {
        // A task file must not bump a NOTE allocation and vice-versa.
        let d = tempdir();
        std::fs::write(d.path().join("PWF-0007.md"), "x").unwrap();
        std::fs::write(d.path().join("PWF-NOTE-0003.md"), "x").unwrap();
        assert_eq!(next_id(&[d.path()], "PWF-NOTE"), "PWF-NOTE-0004");
        assert_eq!(next_id(&[d.path()], "PWF"), "PWF-0008");
    }
}
