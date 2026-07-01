use super::super::model::Item;

#[derive(PartialEq, Eq, Debug, Clone)]
pub(in crate::engines::pending_work) struct OpenItem {
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
}

impl From<Item> for OpenItem {
    fn from(item: Item) -> Self {
        Self {
            id: item.id,
            project: item.project,
            session: item.session,
            prompt: item.prompt,
            repo: item.repo,
            note: item.note,
            item_file: item.item_file,
            line: item.line,
            format: item.format,
            launchable: item.launchable,
            needs_prompt: item.needs_prompt,
            issues: item.issues,
            section: item.section,
            prereq: item.prereq,
            effort: item.effort,
        }
    }
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub(in crate::engines::pending_work) struct ListResult {
    items: Vec<OpenItem>,
    hidden: usize,
}

impl ListResult {
    pub(in crate::engines::pending_work) fn from_items(items: Vec<Item>, hidden: usize) -> Self {
        Self {
            items: items.into_iter().map(OpenItem::from).collect(),
            hidden,
        }
    }

    pub(in crate::engines::pending_work) fn items(&self) -> &[OpenItem] {
        &self.items
    }

    pub(in crate::engines::pending_work) fn hidden(&self) -> usize {
        self.hidden
    }
}
