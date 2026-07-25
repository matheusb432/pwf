use std::fmt;

use thiserror::Error;

/// Stores one canonical lowercase snake-case task label.
///
/// # Examples
///
/// ```
/// use pwf_domain::pending_work::Tag;
///
/// let tag = Tag::try_from("csharp_export").unwrap();
/// assert_eq!(tag.as_ref(), "csharp_export");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Tag(String);

impl TryFrom<&str> for Tag {
    type Error = InvalidTagError;

    fn try_from(raw: &str) -> Result<Self, Self::Error> {
        let valid_chars = raw
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_');
        let valid_separators = !raw.starts_with('_') && !raw.ends_with('_') && !raw.contains("__");
        let has_alphanumeric = raw.chars().any(|ch| ch.is_ascii_alphanumeric());
        let canonical_case = !raw.chars().any(|ch| ch.is_ascii_uppercase());
        if raw.is_empty()
            || !valid_chars
            || !valid_separators
            || !has_alphanumeric
            || !canonical_case
        {
            return Err(InvalidTagError {
                raw: raw.to_string(),
            });
        }
        Ok(Self(raw.to_string()))
    }
}

impl AsRef<str> for Tag {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Stores a non-empty ordered collection of canonical task labels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tags(Vec<Tag>);

impl Tags {
    /// Creates a non-empty ordered collection.
    pub fn try_new(values: Vec<Tag>) -> Result<Self, EmptyTagsError> {
        if values.is_empty() {
            return Err(EmptyTagsError);
        }
        Ok(Self(values))
    }

    /// Iterates over tags in first-seen order.
    pub fn iter(&self) -> impl Iterator<Item = &Tag> {
        self.0.iter()
    }
}

/// Reports a non-canonical tag value.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("invalid canonical tag: {raw:?}")]
pub struct InvalidTagError {
    raw: String,
}

/// Reports an empty canonical tag collection.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
#[error("tags cannot be empty")]
pub struct EmptyTagsError;

#[cfg(test)]
mod tests {
    use super::{Tag, Tags};

    fn values(tags: &Tags) -> Vec<&str> {
        tags.iter().map(AsRef::as_ref).collect()
    }

    #[test]
    fn canonical_tag_and_non_empty_collection_preserve_values() {
        let tags = Tags::try_new(vec![
            Tag::try_from("sqlite").unwrap(),
            Tag::try_from("csharp_export").unwrap(),
        ])
        .unwrap();
        assert_eq!(values(&tags), ["sqlite", "csharp_export"]);
        assert!(Tags::try_new(Vec::new()).is_err());
    }

    #[test]
    fn tag_rejects_noncanonical_values() {
        for raw in ["SQLite", "csharp-export", " sqlite", "_sqlite", "c#"] {
            assert!(Tag::try_from(raw).is_err(), "input: {raw:?}");
        }
    }
}
