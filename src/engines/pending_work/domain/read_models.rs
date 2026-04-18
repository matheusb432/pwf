use super::super::model::Item;

#[derive(PartialEq, Eq, Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prereq: Option<String>,
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
            prereq: item.prereq,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::pending_work::model::Item;

    #[test]
    fn open_item_json_excludes_cursor_and_section_internals() {
        let item = Item {
            id: "GLP-0001".to_string(),
            project: "glep-shimeji".to_string(),
            session: "tray gui".to_string(),
            prompt: "do it".to_string(),
            repo: Some("/repo".to_string()),
            note: "/notes/glep-shimeji.md".to_string(),
            item_file: Some("/notes/GLP-0001.md".to_string()),
            line: 7,
            format: "file".to_string(),
            marker_index: 99,
            marker_length: 12,
            launchable: true,
            needs_prompt: false,
            issues: vec![],
            section: Some("Future".to_string()),
            prereq: Some("\"[[GLP-0002]]\"".to_string()),
        };

        let value = serde_json::to_value(OpenItem::from(item)).unwrap();

        assert_eq!(value["id"], "GLP-0001");
        assert_eq!(value["itemFile"], "/notes/GLP-0001.md");
        assert_eq!(value["prereq"], "\"[[GLP-0002]]\"");
        assert!(value.get("markerIndex").is_none(), "{value}");
        assert!(value.get("markerLength").is_none(), "{value}");
        assert!(value.get("section").is_none(), "{value}");
    }
}
