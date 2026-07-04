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

/// Canonical form of a raw task id. Trims and uppercases, then collapses every
/// accepted shorthand to the canonical `PREFIX-NNNN`: the canonical form itself,
/// the unpadded hyphen form (`pwf-98`), the glued form (`cfg57`), and the
/// hyphenated unpadded code form (`cfg-57`). The prefix is a 2–4 letter code
/// only, so this is config-free. Any other shape is returned trimmed+uppercased
/// so unknown ids still flow downstream to a not-found error, not a parse panic.
pub fn canonical_pending_id(raw: &str) -> String {
    let trimmed = raw.trim().to_ascii_uppercase();
    match crate::regexes::PENDING_ID_COMPACT_RE.captures(&trimmed) {
        Some(caps) => {
            let code = &caps[1];
            // 1–4 digits fit u32; a match guarantees parse success.
            let n: u32 = caps[2].parse().unwrap_or(0);
            format!("{code}-{n:04}")
        }
        None => trimmed,
    }
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
    fn canonical_pending_id_normalizes_all_accepted_forms() {
        // Canonical passes through unchanged (case-insensitive).
        assert_eq!(canonical_pending_id("PWF-0098"), "PWF-0098");
        assert_eq!(canonical_pending_id("pwf-0098"), "PWF-0098");
        assert_eq!(canonical_pending_id("  pwf-0047  "), "PWF-0047");
        // Unpadded hyphen form zero-pads.
        assert_eq!(canonical_pending_id("pwf-98"), "PWF-0098");
        // Glued form splits letters/digits and zero-pads.
        assert_eq!(canonical_pending_id("cfg57"), "CFG-0057");
        assert_eq!(canonical_pending_id("CFG57"), "CFG-0057");
        // Hyphenated unpadded code form.
        assert_eq!(canonical_pending_id("cfg-57"), "CFG-0057");
        // Unrecognised shapes pass through uppercased (flow to not-found).
        assert_eq!(canonical_pending_id("garbage"), "GARBAGE");
        assert_eq!(canonical_pending_id("PWF-0047-extra"), "PWF-0047-EXTRA");
        assert_eq!(canonical_pending_id("toolong-0047"), "TOOLONG-0047");
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
