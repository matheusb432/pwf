/// Selects how one collection changes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum CollectionEdit<T> {
    /// Leaves the stored collection unchanged.
    #[default]
    Unchanged,
    /// Appends values to the stored collection.
    Append(T),
    /// Replaces the stored collection with the supplied values.
    Replace(T),
    /// Removes the stored collection.
    Clear,
}

impl<T> CollectionEdit<T> {
    #[must_use]
    pub fn addition(&self) -> Option<&T> {
        match self {
            Self::Append(value) | Self::Replace(value) => Some(value),
            Self::Unchanged | Self::Clear => None,
        }
    }

    #[must_use]
    pub fn is_unchanged(&self) -> bool {
        matches!(self, Self::Unchanged)
    }
}
