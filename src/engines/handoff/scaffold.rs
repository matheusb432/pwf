//! The markdown template for a freshly-created handoff.

/// Build the markdown content for a new handoff.
pub fn scaffold(title: &str, project: &str, today: &str, pw: Option<&str>) -> String {
    let mut s = String::new();
    s.push_str("---\nstatus: active\n");
    s.push_str(&format!("project: {project}\ncreated: {today}\n"));
    if let Some(p) = pw {
        s.push_str(&format!("pw: {p}\n"));
    }
    s.push_str("---\n\n");
    s.push_str(&format!("# {title}\n\n"));
    s.push_str("## Goals\n- [ ] <task title> :: <task description>\n\n");
    s.push_str("## Context\n\n## Next steps\n-\n\n");
    s.push_str("<!-- Lifecycle: while active, this is a LIVE document \u{2014} check off Goals as you finish them.\n");
    s.push_str("     When all Goals are done run `handoff done <id>` (sets status done, checks the pw item,\n");
    s.push_str("     drops the LEDGER row, moves this file to archived/, commits). Never edit archived/. -->\n");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaffold_without_pw() {
        let s = scaffold("Managed Flow", "test-project", "2026-01-01", None);
        assert!(
            s.starts_with("---\nstatus: active\nproject: test-project\ncreated: 2026-01-01\n---\n")
        );
        assert!(!s.contains("pw:"));
        assert!(s.contains("# Managed Flow"));
        // em-dash in lifecycle comment
        assert!(s.contains('\u{2014}'));
        assert!(s.contains("- [ ] <task title> :: <task description>"));
    }

    #[test]
    fn scaffold_with_pw_inserts_after_created() {
        let s = scaffold(
            "Managed Flow",
            "test-project",
            "2026-01-01",
            Some("TST-0001"),
        );
        assert!(s.contains("created: 2026-01-01\npw: TST-0001\n---"));
    }
}
