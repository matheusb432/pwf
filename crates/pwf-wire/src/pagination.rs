#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CursorPage<Item, Key> {
    pub items: Vec<Item>,
    pub next_key: Option<Key>,
}
