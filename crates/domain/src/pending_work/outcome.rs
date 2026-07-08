use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddedItem {
    pub id: String,
    pub project: String,
    pub title: String,
    pub note_path: PathBuf,
    pub created_section: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovedItem {
    pub id: String,
    pub project: String,
    pub title: String,
    pub deleted_path: PathBuf,
    pub unlinked: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdatedItem {
    OpenItemEdit {
        id: String,
        project: String,
        title: String,
    },
    Changed {
        id: String,
        changes: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MutationOutcome {
    Added(AddedItem),
    Removed(RemovedItem),
    Updated(UpdatedItem),
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn mutation_outcome_preserves_added_item_payload() {
        let item = AddedItem {
            id: "PWF-0001".to_string(),
            project: "pwf".to_string(),
            title: "do it".to_string(),
            note_path: PathBuf::from("/x/PWF-0001.md"),
            created_section: Some("Human".to_string()),
        };

        assert_eq!(
            MutationOutcome::Added(item.clone()),
            MutationOutcome::Added(item)
        );
    }

    #[test]
    fn mutation_outcome_preserves_removed_item_payload() {
        let item = RemovedItem {
            id: "PWF-0002".to_string(),
            project: "pwf".to_string(),
            title: "stale task".to_string(),
            deleted_path: PathBuf::from("/x.md"),
            unlinked: "/x/pwf.md".to_string(),
        };

        assert_eq!(
            MutationOutcome::Removed(item.clone()),
            MutationOutcome::Removed(item)
        );
    }

    #[test]
    fn mutation_outcome_preserves_updated_item_payload() {
        let item = UpdatedItem::Changed {
            id: "PWF-0003".to_string(),
            changes: vec!["report appended".to_string()],
        };

        assert_eq!(
            MutationOutcome::Updated(item.clone()),
            MutationOutcome::Updated(item)
        );
    }
}
