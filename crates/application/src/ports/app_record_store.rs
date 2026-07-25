/// Defines the scope, identity, insertion, and patch types for a persisted record.
pub trait Record {
    type Scope;
    type Id;
    type New;
    type Patch;
}

/// Persists records by scope and identity without filtering, ordering, or pagination.
pub trait AppRecordStore<R: Record>: Clone + Send + Sync + 'static {
    type Error: std::error::Error + Send + Sync + 'static;

    /// Reads one record by scope and identity.
    ///
    /// # Errors
    ///
    /// Returns the adapter error when the record cannot be read.
    fn get(&self, scope: &R::Scope, id: &R::Id) -> Result<Option<R>, Self::Error>;
    /// Reads every record within `scope`, unfiltered and unordered.
    ///
    /// # Errors
    ///
    /// Returns the adapter error when the records cannot be read.
    fn list(&self, scope: &R::Scope) -> Result<Vec<R>, Self::Error>;
    /// Inserts one record within `scope`.
    ///
    /// # Errors
    ///
    /// Returns the adapter error when the record cannot be inserted.
    fn insert(&self, scope: &R::Scope, new: R::New) -> Result<R, Self::Error>;
    /// Applies one patch to the identified record.
    ///
    /// # Errors
    ///
    /// Returns the adapter error when the record cannot be updated.
    fn update(&self, scope: &R::Scope, id: &R::Id, patch: R::Patch) -> Result<(), Self::Error>;
    /// Deletes the identified record.
    ///
    /// # Errors
    ///
    /// Returns the adapter error when the record cannot be deleted.
    fn delete(&self, scope: &R::Scope, id: &R::Id) -> Result<(), Self::Error>;
}
