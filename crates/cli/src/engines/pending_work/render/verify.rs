use std::fmt::Write;

use crate::engines::pending_work::verify::VerifySessionOk;

pub(in crate::engines::pending_work) fn render_verify(outcome: &VerifySessionOk) -> String {
    let result = if outcome.probe.available && outcome.launchable {
        "pass"
    } else {
        "fail"
    };

    let mut out = match outcome.task_id.as_deref() {
        Some(i) => format!("# verify {i} \u{2014} {result}\n"),
        None => format!("# verify \u{2014} {result}\n"),
    };
    if outcome.probe.available {
        let version = outcome
            .probe
            .version
            .as_deref()
            .map(|v| format!(" ({v})"))
            .unwrap_or_default();
        let path = outcome
            .probe
            .path
            .as_deref()
            .map(|p| format!(" at {p}"))
            .unwrap_or_default();
        let _ = writeln!(out, "{}: available{version}{path}", outcome.probe.binary);
    } else {
        let _ = writeln!(out, "{}: not found on PATH", outcome.probe.binary);
    }
    let _ = writeln!(
        out,
        "launchable: {}",
        if outcome.launchable { "yes" } else { "no" }
    );
    let _ = writeln!(out, "command: {}", outcome.command_preview);
    if outcome.issues.is_empty() {
        out.push_str("issues: none\n");
    } else {
        out.push_str("issues:\n");
        for iss in &outcome.issues {
            let _ = writeln!(out, "- {iss}");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use pwf_infra::session::AgentProbe;

    use super::*;
    use crate::engines::pending_work::verify::VerifySessionOk;

    fn outcome(binary: &str, available: bool, launchable: bool) -> VerifySessionOk {
        VerifySessionOk {
            task_id: Some("PWF-0001".to_string()),
            probe: AgentProbe {
                binary: binary.to_string(),
                available,
                path: available.then(|| format!("/usr/bin/{binary}")),
                version: available.then(|| "1.2.3".to_string()),
            },
            launchable,
            issues: Vec::new(),
            command_preview: format!("{binary} -- \"Pending-work ID: PWF-0001…\""),
        }
    }

    #[test]
    fn verify_text_pass_has_result_in_heading() {
        let out = render_verify(&outcome("claude", true, true));
        assert!(out.starts_with("# verify PWF-0001 \u{2014} pass"));
        assert!(out.contains("claude: available (1.2.3) at /usr/bin/claude"));
        assert!(out.contains("launchable: yes"));
        assert!(out.contains("command: "));
        assert!(out.contains("issues: none"));
    }

    #[test]
    fn verify_text_fail_lists_issues() {
        let mut result = outcome("claude", false, false);
        result.task_id = Some("PWF-0002".to_string());
        result.issues.push("Prompt is a placeholder".to_string());
        let out = render_verify(&result);
        assert!(out.starts_with("# verify PWF-0002 \u{2014} fail"));
        assert!(out.contains("claude: not found on PATH"));
        assert!(out.contains("issues:\n"));
        assert!(out.contains("- Prompt is a placeholder"));
    }

    #[test]
    fn verify_text_no_item_pass_when_agent_present() {
        let mut result = outcome("claude", true, true);
        result.task_id = None;
        result.probe.path = Some("/bin/claude".into());
        result.command_preview = "claude".to_string();
        let out = render_verify(&result);
        assert!(out.starts_with("# verify \u{2014} pass"), "heading: {out}");
        assert!(out.contains("claude: available (1.2.3) at /bin/claude"));
        assert!(out.contains("launchable: yes"));
    }

    #[test]
    fn verify_text_no_item_fail_when_agent_missing() {
        let mut result = outcome("claude", false, true);
        result.task_id = None;
        result.command_preview = "claude".to_string();
        let out = render_verify(&result);
        assert!(out.starts_with("# verify \u{2014} fail"), "heading: {out}");
        assert!(out.contains("claude: not found on PATH"));
    }

    #[test]
    fn verify_text_fails_and_lists_issue_on_broken_model_tiers() {
        let mut result = outcome("claude", true, false);
        result.task_id = Some("PWF-0002".to_string());
        result
            .issues
            .push("tier highest has no claude_model set".to_string());
        let out = render_verify(&result);
        assert!(
            out.starts_with("# verify PWF-0002 \u{2014} fail"),
            "got: {out}"
        );
        assert!(out.contains("launchable: no"));
        assert!(out.contains("- tier highest has no claude_model set"));
    }
}
