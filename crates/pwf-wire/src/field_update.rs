/// Selects how one resource field changes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum FieldUpdate<T> {
    /// Leaves the stored field unchanged.
    #[default]
    Unchanged,
    /// Replaces the stored field.
    Update(T),
    /// Removes the stored field.
    Clear,
}

impl<T> FieldUpdate<T> {
    pub(crate) fn is_unchanged(&self) -> bool {
        matches!(self, Self::Unchanged)
    }
}
