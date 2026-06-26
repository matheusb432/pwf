//! `pwf verify` rendering: probe a selected agent and report launchability as markdown.

use super::probe::AgentProbe;
use crate::engines::pending_work::{launch::LaunchPolicy, model::Item, session::AgentLauncher};

/// Render an agent argv as a single shell-ish command line, truncating a multi-line
/// trailing prompt to its first line so `verify` stays one screen. Works for any argv
/// (claude's `--name`-shaped vector and codex's `--`-guarded one alike).
fn command_line(argv: &[String]) -> String {
    let Some((last, head)) = argv.split_last() else {
        return String::new();
    };
    let mut parts: Vec<String> = head
        .iter()
        .map(|a| {
            if a.contains(' ') {
                format!("\"{a}\"")
            } else {
                a.clone()
            }
        })
        .collect();
    let first = last.lines().next().unwrap_or("");
    let shown = if last.lines().nth(1).is_some() {
        format!("{first}\u{2026}")
    } else {
        first.to_string()
    };
    parts.push(format!("\"{shown}\""));
    parts.join(" ")
}

/// Render the verify result as markdown for `item` under `launcher`, probed by `probe`.
/// The `pass`/`fail` token sits in the heading so an agent can branch on one read. The
/// command line is sourced from `launcher.argv`, so it always reflects what `pwf
/// session` would actually run for the selected agent.
pub fn verify_text_with_probe(
    item: Option<&Item>,
    launcher: &dyn AgentLauncher,
    probe: &dyn AgentProbe,
) -> String {
    let binary = launcher.binary();
    let (id, launchable, issues, display) = match item {
        Some(it) => (
            Some(it.id.as_str()),
            it.launchable,
            it.issues.clone(),
            command_line(&launcher.argv(it, LaunchPolicy::default())),
        ),
        // No item → no concrete command; show the binary that would run.
        None => (None, true, vec![], binary.to_string()),
    };
    let result = if probe.available() && launchable {
        "pass"
    } else {
        "fail"
    };

    let mut out = match id {
        Some(i) => format!("# verify {i} \u{2014} {result}\n"),
        None => format!("# verify \u{2014} {result}\n"),
    };
    if probe.available() {
        let ver = probe
            .version()
            .map(|v| format!(" ({v})"))
            .unwrap_or_default();
        let path = probe.path().map(|p| format!(" at {p}")).unwrap_or_default();
        out.push_str(&format!("{binary}: available{ver}{path}\n"));
    } else {
        out.push_str(&format!("{binary}: not found on PATH\n"));
    }
    out.push_str(&format!(
        "launchable: {}\n",
        if launchable { "yes" } else { "no" }
    ));
    out.push_str(&format!("command: {display}\n"));
    if issues.is_empty() {
        out.push_str("issues: none\n");
    } else {
        out.push_str("issues:\n");
        for iss in &issues {
            out.push_str(&format!("- {iss}\n"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::pending_work::{
        agent::probe::FakeProbe,
        model::Item,
        session::{ClaudeLauncher, CodexLauncher},
    };

    #[test]
    fn verify_text_pass_has_result_in_heading() {
        let probe = FakeProbe {
            available: true,
            path: Some("/usr/bin/claude".to_string()),
            version: Some("1.2.3".to_string()),
        };
        let item = Item {
            launchable: true,
            issues: vec![],
            ..Item::default_for_test("PWF-0001", "cli launch")
        };
        let out = verify_text_with_probe(Some(&item), &ClaudeLauncher, &probe);
        assert!(out.starts_with("# verify PWF-0001 \u{2014} pass"));
        assert!(out.contains("claude: available (1.2.3) at /usr/bin/claude"));
        assert!(out.contains("launchable: yes"));
        assert!(out.contains("command: "));
        assert!(out.contains("issues: none"));
    }

    #[test]
    fn verify_text_fail_lists_issues() {
        let probe = FakeProbe {
            available: false,
            path: None,
            version: None,
        };
        let item = Item {
            launchable: false,
            issues: vec!["Prompt is a placeholder".to_string()],
            ..Item::default_for_test("PWF-0002", "broken")
        };
        let out = verify_text_with_probe(Some(&item), &ClaudeLauncher, &probe);
        assert!(out.starts_with("# verify PWF-0002 \u{2014} fail"));
        assert!(out.contains("claude: not found on PATH"));
        assert!(out.contains("issues:\n"));
        assert!(out.contains("- Prompt is a placeholder"));
    }

    #[test]
    fn verify_text_available_claude_non_launchable_item_fails() {
        let probe = FakeProbe {
            available: true,
            path: Some("/usr/bin/claude".to_string()),
            version: Some("1.2.3".to_string()),
        };
        let item = Item {
            launchable: false,
            issues: vec!["Prompt is a placeholder".to_string()],
            ..Item::default_for_test("PWF-0003", "non-launchable")
        };
        let out = verify_text_with_probe(Some(&item), &ClaudeLauncher, &probe);
        assert!(out.starts_with("# verify PWF-0003 \u{2014} fail"));
        assert!(out.contains("launchable: no"));
        assert!(out.contains("command: "));
    }

    #[test]
    fn verify_text_renders_codex_binary_and_command() {
        let probe = FakeProbe {
            available: true,
            path: Some("/usr/bin/codex".to_string()),
            version: Some("0.142.0".to_string()),
        };
        let item = Item {
            launchable: true,
            issues: vec![],
            ..Item::default_for_test("PWF-0068", "codex verify")
        };
        let out = verify_text_with_probe(Some(&item), &CodexLauncher, &probe);
        assert!(out.starts_with("# verify PWF-0068 \u{2014} pass"));
        assert!(out.contains("codex: available (0.142.0) at /usr/bin/codex"));
        assert!(out.contains("command: "));
        assert!(out.contains("__codex-thread-title"));
        assert!(out.contains("PWF-0068 - codex verify"));
        assert!(out.contains(" codex -- "));
        assert!(out.contains("launchable: yes"));
    }

    #[test]
    fn verify_text_no_item_pass_when_agent_present() {
        let probe = FakeProbe {
            available: true,
            path: Some("/bin/claude".into()),
            version: Some("1.2.3".into()),
        };
        let out = verify_text_with_probe(None, &ClaudeLauncher, &probe);
        assert!(out.starts_with("# verify \u{2014} pass"), "heading: {out}");
        assert!(out.contains("claude: available (1.2.3) at /bin/claude"));
        assert!(out.contains("launchable: yes"));
    }

    #[test]
    fn verify_text_no_item_fail_when_agent_missing() {
        let probe = FakeProbe {
            available: false,
            path: None,
            version: None,
        };
        let out = verify_text_with_probe(None, &ClaudeLauncher, &probe);
        assert!(out.starts_with("# verify \u{2014} fail"), "heading: {out}");
        assert!(out.contains("claude: not found on PATH"));
    }

    #[test]
    fn verify_text_no_item_shows_binary_only_command() {
        let probe = FakeProbe {
            available: true,
            path: Some("/usr/bin/codex".to_string()),
            version: Some("0.142.0".to_string()),
        };
        let out = verify_text_with_probe(None, &CodexLauncher, &probe);
        assert!(out.starts_with("# verify \u{2014} pass"));
        assert!(out.contains("command: codex\n"));
    }
}
