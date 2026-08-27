use serde::{Deserialize, Serialize};

use super::fixture::{DocumentSize, document_body};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct NodeMetadata {
    pub node_id: u64,
    pub title: String,
    pub status: String,
    pub related: Vec<String>,
    pub tags: Vec<String>,
    pub created: String,
}

pub fn node_metadata() -> NodeMetadata {
    NodeMetadata {
        node_id: 42,
        title: "Harbor signal index".to_string(),
        status: "active".to_string(),
        related: vec![
            "[[Signal Station]]".to_string(),
            "[[Northern Harbor]]".to_string(),
            "[[PWF-0001]]".to_string(),
        ],
        tags: vec!["benchmark".to_string(), "obsidian".to_string()],
        created: "2026-08-27".to_string(),
    }
}

pub fn node_source(size: DocumentSize) -> String {
    format!(
        concat!(
            "---\n",
            "node_id: 42\n",
            "title: Harbor signal index\n",
            "status: active\n",
            "related: [\"[[Signal Station]]\", \"[[Northern Harbor]]\", \"[[PWF-0001]]\"]\n",
            "tags: [\"benchmark\", \"obsidian\"]\n",
            "created: 2026-08-27\n",
            "---\n\n",
            "# Harbor signal index\n\n",
            "{}"
        ),
        document_body(size),
    )
}
