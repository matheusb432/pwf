use std::convert::Infallible;

use pwf_domain::pending_work::{OpenItem, ProjectName};

use crate::ports::PendingWorkReadStore;

#[derive(Debug, Clone, Default)]
pub struct InMemoryPendingWorkReadStore {
    items: Vec<OpenItem>,
}

impl InMemoryPendingWorkReadStore {
    pub fn with_items(items: Vec<OpenItem>) -> Self {
        Self { items }
    }
}

impl PendingWorkReadStore for InMemoryPendingWorkReadStore {
    type Error = Infallible;

    fn open_items_for_project(&self, project: &ProjectName) -> Result<Vec<OpenItem>, Self::Error> {
        Ok(self
            .items
            .iter()
            .filter(|item| item.project == project.as_ref())
            .cloned()
            .collect())
    }

    fn all_open_items(&self) -> Result<Vec<OpenItem>, Self::Error> {
        Ok(self.items.clone())
    }

    fn open_item(&self, id: &str) -> Result<OpenItem, Self::Error> {
        Ok(self
            .items
            .iter()
            .find(|item| item.id.eq_ignore_ascii_case(id))
            .expect("in-memory test query references a staged item")
            .clone())
    }
}
