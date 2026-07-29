use nutype::nutype;

#[nutype(
    validate(predicate = is_canonical_pending_id),
    derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, AsRef, Display,)
)]
pub struct WorkItemId(String);

fn is_canonical_pending_id(raw: &str) -> bool {
    let Some((code, digits)) = raw.split_once('-') else {
        return false;
    };
    (2..=4).contains(&code.len())
        && code.chars().all(|ch| ch.is_ascii_uppercase())
        && digits.len() == 4
        && digits.chars().all(|ch| ch.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_item_id_accepts_canonical_values() {
        assert_eq!(
            WorkItemId::try_new("PWF-0047").unwrap().as_ref(),
            "PWF-0047"
        );
    }

    #[test]
    fn work_item_id_rejects_noncanonical_shapes() {
        assert!(WorkItemId::try_new("pwf-0047").is_err());
        assert!(WorkItemId::try_new("PWF-47").is_err());
        assert!(WorkItemId::try_new("cfg57").is_err());
        assert!(WorkItemId::try_new("PWF-0047-extra").is_err());
        assert!(WorkItemId::try_new("TOOLONG-0047").is_err());
        assert!(WorkItemId::try_new("CFG-99999").is_err());
        assert!(WorkItemId::try_new("nope").is_err());
    }
}
