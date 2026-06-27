//! Sequential id allocation by scanning a project dir for `{KEY}-NNNN.md` files.

use std::path::Path;

use regex::Regex;

/// Allocate `"{key}-{max+1:04}"` by scanning every dir in `dirs` for `*.md`
/// files whose stem matches `^{key}-(\d{4})$`. Missing dirs are skipped; gaps
/// are preserved (allocation is `max + 1`, never a gap-fill).
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

    fn tempdir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("pwf_core_id_{tag}_{}", nanos()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }
    fn nanos() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    }

    #[test]
    fn allocates_one_when_empty() {
        let d = tempdir("empty");
        assert_eq!(next_id(&[d.as_path()], "PWF"), "PWF-0001");
    }

    #[test]
    fn allocates_max_plus_one_across_dirs() {
        let d = tempdir("max");
        let arch = d.join("_archive");
        std::fs::create_dir_all(&arch).unwrap();
        std::fs::write(d.join("PWF-0002.md"), "x").unwrap();
        std::fs::write(arch.join("PWF-0005.md"), "x").unwrap();
        assert_eq!(next_id(&[d.as_path(), arch.as_path()], "PWF"), "PWF-0006");
    }

    #[test]
    fn disjoint_keys_do_not_perturb_each_other() {
        // A task file must not bump a NOTE allocation and vice-versa.
        let d = tempdir("disjoint");
        std::fs::write(d.join("PWF-0007.md"), "x").unwrap();
        std::fs::write(d.join("PWF-NOTE-0003.md"), "x").unwrap();
        assert_eq!(next_id(&[d.as_path()], "PWF-NOTE"), "PWF-NOTE-0004");
        assert_eq!(next_id(&[d.as_path()], "PWF"), "PWF-0008");
    }
}
