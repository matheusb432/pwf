#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenItem {
    pub id: String,
    pub project: String,
    pub session: String,
    pub prompt: String,
    pub repo: Option<String>,
    pub note: String,
    pub item_file: Option<String>,
    pub line: usize,
    pub format: String,
    pub launchable: bool,
    pub needs_prompt: bool,
    pub issues: Vec<String>,
    pub section: Option<String>,
    pub prereq: Option<String>,
    pub effort: Option<String>,
    pub tags: Option<String>,
    pub created: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListResult {
    pub items: Vec<OpenItem>,
    pub hidden: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListScope {
    Default,
    HumanOnly,
    FutureOnly,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderField {
    Created,
    Id,
    ProjectId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderDirection {
    Asc,
    Desc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrderSpec {
    pub field: OrderField,
    pub direction: OrderDirection,
}

impl Default for OrderSpec {
    fn default() -> Self {
        Self {
            field: OrderField::Created,
            direction: OrderDirection::Desc,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListLimit(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffortFilter(pub u8);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_order_is_created_desc() {
        assert_eq!(
            OrderSpec::default(),
            OrderSpec {
                field: OrderField::Created,
                direction: OrderDirection::Desc,
            }
        );
    }

    #[test]
    fn list_result_keeps_items_and_hidden_count() {
        let result = ListResult {
            items: vec![OpenItem {
                id: "PWF-0001".to_string(),
                project: "pwf".to_string(),
                session: "session".to_string(),
                prompt: "prompt".to_string(),
                repo: Some("/repo".to_string()),
                note: "note".to_string(),
                item_file: Some("/note.md".to_string()),
                line: 12,
                format: "file".to_string(),
                launchable: true,
                needs_prompt: false,
                issues: vec!["issue".to_string()],
                section: Some("Human".to_string()),
                prereq: Some("[[CFG-0001]]".to_string()),
                effort: Some("2".to_string()),
                tags: None,
                created: Some("2026-07-06".to_string()),
            }],
            hidden: 3,
        };

        assert_eq!(result.items.len(), 1);
        assert_eq!(result.hidden, 3);
    }
}
