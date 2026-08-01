use nutype::nutype;

use crate::project::ProjectId;

#[nutype(
    validate(predicate = is_canonical_pending_id),
    derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, AsRef, Display,)
)]
pub struct WorkItemId(String);

impl WorkItemId {
    /// Returns the validated three-letter project prefix.
    ///
    /// # Panics
    ///
    /// Panics only if the `WorkItemId` invariant was bypassed internally.
    pub fn project_id(&self) -> ProjectId {
        let (code, _) = self
            .as_ref()
            .split_once('-')
            .expect("WorkItemId validation requires a separator");
        ProjectId::try_new(code).expect("WorkItemId validation requires a canonical project ID")
    }
}

fn is_canonical_pending_id(raw: &str) -> bool {
    let Some((code, digits)) = raw.split_once('-') else {
        return false;
    };
    let Ok(project_id) = ProjectId::try_new(code) else {
        return false;
    };
    project_id.as_ref() == code && digits.len() == 4 && digits.chars().all(|ch| ch.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_item_id_accepts_canonical_values() {
        let id = WorkItemId::try_new("PWF-0047").unwrap();

        assert_eq!(id.as_ref(), "PWF-0047");
        assert_eq!(id.project_id(), ProjectId::try_new("PWF").unwrap());
    }

    #[test]
    fn work_item_id_rejects_noncanonical_shapes() {
        assert!(WorkItemId::try_new("pwf-0047").is_err());
        assert!(WorkItemId::try_new("PW-0047").is_err());
        assert!(WorkItemId::try_new("TOOL-0047").is_err());
        assert!(WorkItemId::try_new("PWF-47").is_err());
        assert!(WorkItemId::try_new("cfg57").is_err());
        assert!(WorkItemId::try_new("PWF-0047-extra").is_err());
        assert!(WorkItemId::try_new("TOOLONG-0047").is_err());
        assert!(WorkItemId::try_new("CFG-99999").is_err());
        assert!(WorkItemId::try_new("nope").is_err());
    }
}
