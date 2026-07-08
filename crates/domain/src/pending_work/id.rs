use nutype::nutype;

#[nutype(
    sanitize(with = |raw: String| collapse_pending_id(&raw)),
    validate(predicate = is_canonical_pending_id),
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
    validate(predicate = is_project_prefix),
    derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, AsRef, Display,)
)]
pub struct ProjectPrefix(String);

fn collapse_pending_id(raw: &str) -> String {
    let trimmed = raw.trim().to_ascii_uppercase();
    match split_compact_pending_id(&trimmed) {
        Some((code, n)) => format!("{code}-{n:04}"),
        None => trimmed,
    }
}

fn split_compact_pending_id(raw: &str) -> Option<(&str, u32)> {
    let digit_start = raw.find(|ch: char| ch.is_ascii_digit())?;
    let (code, digits) = raw.split_at(digit_start);
    let code = code.strip_suffix('-').unwrap_or(code);
    if !(2..=4).contains(&code.len()) || !code.chars().all(|ch| ch.is_ascii_uppercase()) {
        return None;
    }
    if digits.is_empty() || digits.len() > 4 || !digits.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    Some((code, digits.parse().ok()?))
}

fn is_canonical_pending_id(raw: &str) -> bool {
    let Some((code, digits)) = raw.split_once('-') else {
        return false;
    };
    (2..=4).contains(&code.len())
        && code.chars().all(|ch| ch.is_ascii_uppercase())
        && digits.len() == 4
        && digits.chars().all(|ch| ch.is_ascii_digit())
}

fn is_project_prefix(raw: &str) -> bool {
    (2..=4).contains(&raw.len()) && raw.chars().all(|ch| ch.is_ascii_uppercase())
}

pub fn canonical_pending_id(raw: &str) -> String {
    collapse_pending_id(raw)
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
    fn work_item_id_normalizes_shorthand_forms() {
        for (raw, want) in [
            ("PWF-47", "PWF-0047"),
            ("pwf-98", "PWF-0098"),
            ("cfg57", "CFG-0057"),
            ("CFG-57", "CFG-0057"),
        ] {
            assert_eq!(WorkItemId::try_new(raw).unwrap().as_ref(), want);
        }
    }

    #[test]
    fn work_item_id_rejects_noncanonical_shapes() {
        assert!(WorkItemId::try_new("PWF-0047-extra").is_err());
        assert!(WorkItemId::try_new("TOOLONG-0047").is_err());
        assert!(WorkItemId::try_new("CFG-99999").is_err());
        assert!(WorkItemId::try_new("nope").is_err());
    }

    #[test]
    fn canonical_pending_id_normalizes_all_accepted_forms() {
        assert_eq!(canonical_pending_id("PWF-0098"), "PWF-0098");
        assert_eq!(canonical_pending_id("pwf-0098"), "PWF-0098");
        assert_eq!(canonical_pending_id("  pwf-0047  "), "PWF-0047");
        assert_eq!(canonical_pending_id("pwf-98"), "PWF-0098");
        assert_eq!(canonical_pending_id("cfg57"), "CFG-0057");
        assert_eq!(canonical_pending_id("CFG57"), "CFG-0057");
        assert_eq!(canonical_pending_id("cfg-57"), "CFG-0057");
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
}
