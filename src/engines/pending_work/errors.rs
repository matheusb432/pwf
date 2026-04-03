// Shared, interpolated error messages used across more than one submodule.
// Plain (non-interpolated) messages local to a single module live as a `const`
// in that module instead.

/// The canonical create form. Pointed at by the route guards that reject the
/// removed silent-create paths (PWF-0034).
pub(super) const ADD_HINT: &str = r#"Use: pwf pw add <project> "<prompt>""#;

/// An unrecognized `--section` value.
pub(super) fn bad_section(value: &str) -> String {
    format!("Unknown --section value '{value}'. Use one of: future, human, low-prio.")
}

/// A project name resolved to no repo mapping in config/pending-work.json.
pub(super) fn not_mapped_to_repo(project: &str) -> String {
    format!("Project '{project}' is not mapped to a repo in config/pending-work.json.")
}

/// An item is not launchable; lists the blocking issues.
pub(super) fn not_launchable(id: &str, issues: &[String]) -> String {
    format!(
        "Pending-work item '{}' is not launchable: {}",
        id,
        issues.join("; ")
    )
}
