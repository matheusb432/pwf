//! Agent-binary probe: PATH resolution and version checks. Generic over the binary
//! name so `pwf verify`/`pwf session` can probe claude or codex.

pub trait AgentProbe {
    fn available(&self) -> bool;
    fn path(&self) -> Option<&str>;
    fn version(&self) -> Option<&str>;
}

/// Real probe: resolves `binary` on PATH and queries its `--version`.
pub struct RealProbe {
    pub available: bool,
    pub path: Option<String>,
    pub version: Option<String>,
}

impl RealProbe {
    pub fn resolve(binary: &str) -> Self {
        let (available, path, version) = match which_binary(binary) {
            None => (false, None, None),
            Some(p) => {
                let ver = run_version(&p);
                (true, Some(p), ver)
            }
        };
        RealProbe {
            available,
            path,
            version,
        }
    }
}

/// Resolve `name` on PATH (`where` on Windows, `command -v` on Unix). `name` is a
/// launcher-owned constant (`"claude"`/`"codex"`), never store data — no injection.
pub(in crate::engines::pending_work) fn which_binary(name: &str) -> Option<String> {
    #[cfg(windows)]
    {
        let output = std::process::Command::new("where")
            .arg(name)
            .output()
            .ok()?;
        if output.status.success() {
            let s = String::from_utf8_lossy(&output.stdout);
            return s.lines().next().map(|l| l.trim().to_string());
        }
        None
    }
    #[cfg(not(windows))]
    {
        // `command -v` is a shell builtin → run it under `sh`. Prints the resolved
        // path on success; uses pwf's inherited PATH (best-effort preflight).
        let output = std::process::Command::new("sh")
            .args(["-c", &format!("command -v {name}")])
            .output()
            .ok()?;
        if output.status.success() {
            let s = String::from_utf8_lossy(&output.stdout);
            let first = s.lines().next()?.trim();
            if !first.is_empty() {
                return Some(first.to_string());
            }
        }
        None
    }
}

fn run_version(path: &str) -> Option<String> {
    let output = std::process::Command::new(path)
        .arg("--version")
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&output.stdout);
    s.lines().next().map(|l| l.trim().to_string())
}

impl AgentProbe for RealProbe {
    fn available(&self) -> bool {
        self.available
    }
    fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }
    fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }
}

/// Scriptable probe for unit tests.
#[cfg(test)]
pub struct FakeProbe {
    pub available: bool,
    pub path: Option<String>,
    pub version: Option<String>,
}

#[cfg(test)]
impl AgentProbe for FakeProbe {
    fn available(&self) -> bool {
        self.available
    }
    fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }
    fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_probe_reports_its_fields() {
        let p = FakeProbe {
            available: true,
            path: Some("/usr/bin/codex".to_string()),
            version: Some("0.142.0".to_string()),
        };
        assert!(p.available());
        assert_eq!(p.path(), Some("/usr/bin/codex"));
        assert_eq!(p.version(), Some("0.142.0"));
    }

    #[test]
    fn which_binary_resolves_a_real_program_and_rejects_a_fake() {
        // `sh` exists on every Unix test host; on Windows `cmd` resolves via `where`.
        #[cfg(unix)]
        assert!(which_binary("sh").is_some());
        assert!(which_binary("definitely-not-a-real-binary-xyz").is_none());
    }
}
