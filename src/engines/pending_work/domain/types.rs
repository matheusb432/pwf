use nutype::nutype;

#[nutype(
    sanitize(trim, uppercase),
    validate(regex = "^[A-Z]{2,4}-\\d{4}$"),
    derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, AsRef, Display,)
)]
pub struct WorkItemId(String);

#[nutype(
    sanitize(trim),
    validate(not_empty),
    derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, AsRef, Display,)
)]
pub struct ProjectName(String);

#[nutype(
    sanitize(trim, uppercase),
    validate(regex = "^[A-Z]{2,4}$"),
    derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, AsRef, Display,)
)]
pub struct ProjectPrefix(String);

#[nutype(
    sanitize(trim, with = |raw: String| crate::engines::pending_work::text::normalize_title(&raw)),
    validate(not_empty),
    derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, AsRef, Display),
)]
pub struct TaskTitle(String);

/// Canonical form of a raw task id: the validated `WorkItemId` rendering when the
/// shape parses (case-insensitive), else the trimmed-uppercased input so unknown
/// shapes still flow downstream to a not-found error rather than a parse failure.
pub fn canonical_pending_id(raw: &str) -> String {
    WorkItemId::try_new(raw)
        .map(|id| id.to_string())
        .unwrap_or_else(|_| raw.trim().to_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_item_id_normalizes_lowercase_prefix() {
        let id = WorkItemId::try_new("pwf-0047").unwrap();
        assert_eq!(id.as_ref(), "PWF-0047");
    }

    #[test]
    fn work_item_id_rejects_noncanonical_shapes() {
        assert!(WorkItemId::try_new("PWF-47").is_err());
        assert!(WorkItemId::try_new("PWF-0047-extra").is_err());
        assert!(WorkItemId::try_new("TOOLONG-0047").is_err());
        assert!(WorkItemId::try_new("nope").is_err());
    }

    #[test]
    fn canonical_pending_id_normalizes_valid_and_passes_through_invalid() {
        assert_eq!(canonical_pending_id("pwf-0047"), "PWF-0047");
        assert_eq!(canonical_pending_id("  pwf-0047  "), "PWF-0047");
        assert_eq!(canonical_pending_id("garbage"), "GARBAGE");
    }

    #[test]
    fn project_name_trims_and_rejects_blank() {
        assert_eq!(
            ProjectName::try_new("  glep-shimeji  ").unwrap().as_ref(),
            "glep-shimeji"
        );
        assert!(ProjectName::try_new(" \t ").is_err());
    }

    #[test]
    fn project_prefix_uppercases_and_preserves_width_rules() {
        assert_eq!(ProjectPrefix::try_new("pwf").unwrap().as_ref(), "PWF");
        assert!(ProjectPrefix::try_new("P").is_err());
        assert!(ProjectPrefix::try_new("TOOLONG").is_err());
    }

    #[test]
    fn task_title_trims_lowercases_and_rejects_blank() {
        assert_eq!(
            TaskTitle::try_new("  Fix The THING  ").unwrap().as_ref(),
            "fix the thing"
        );
        assert!(TaskTitle::try_new(" \n ").is_err());
    }
}
