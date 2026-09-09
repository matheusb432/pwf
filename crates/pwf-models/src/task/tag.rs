use std::{fmt, str::FromStr};

use thiserror::Error;

/// Stores one lowercase snake-case task tag.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Tag(String);

impl TryFrom<&str> for Tag {
    type Error = InvalidTagError;

    fn try_from(raw: &str) -> Result<Self, Self::Error> {
        Self::try_from(raw.to_string())
    }
}

impl TryFrom<String> for Tag {
    type Error = InvalidTagError;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        let valid_chars = raw
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_');
        let valid_separators = !raw.starts_with('_') && !raw.ends_with('_') && !raw.contains("__");
        let has_alphanumeric = raw.chars().any(|ch| ch.is_ascii_alphanumeric());
        let is_lowercase = !raw.chars().any(|ch| ch.is_ascii_uppercase());
        if raw.is_empty() || !valid_chars || !valid_separators || !has_alphanumeric || !is_lowercase
        {
            return Err(InvalidTagError { raw });
        }
        Ok(Self(raw))
    }
}

impl Tag {
    #[must_use]
    pub fn into_string(self) -> String {
        self.0
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

/// Contains the normalized tags parsed from one authored argument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagInput(TaskTags);

impl TagInput {
    /// Returns the parsed tags in encounter order.
    fn iter(&self) -> impl Iterator<Item = &Tag> {
        self.0.iter()
    }
}

impl FromStr for TagInput {
    type Err = TagInputError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let segments = value.split(',').collect::<Vec<_>>();
        if segments.iter().any(|raw| raw.trim().is_empty()) {
            return Err(TagInputError::MissingTag {
                raw: value.to_string(),
            });
        }

        let mut tags = Vec::new();
        for raw in segments {
            let normalized = raw.trim().to_ascii_lowercase().replace('-', "_");
            let tag = Tag::try_from(normalized).map_err(|_| TagInputError::InvalidTag {
                raw: raw.to_string(),
            })?;
            push_unseen_tag(&mut tags, tag);
        }
        TaskTags::try_new(tags)
            .map(Self)
            .map_err(|_| TagInputError::MissingTag {
                raw: value.to_string(),
            })
    }
}

/// Stores a non-empty ordered collection of task tags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskTags(Vec<Tag>);

impl TaskTags {
    /// Parses the persisted inline tag sequence.
    ///
    /// # Errors
    ///
    /// Rejects an empty sequence, malformed brackets, or invalid tags.
    pub fn parse_frontmatter(raw: &str) -> Result<Self, ParseTaskTagsError> {
        let trimmed = raw.trim();
        let Some(inner) = trimmed
            .strip_prefix('[')
            .and_then(|value| value.strip_suffix(']'))
        else {
            return Err(ParseTaskTagsError::InvalidFrontmatter {
                raw: raw.to_string(),
            });
        };
        if inner.trim().is_empty() {
            return Err(ParseTaskTagsError::MissingTag {
                raw: raw.to_string(),
            });
        }
        let input = inner.parse::<TagInput>().map_err(|error| match error {
            TagInputError::MissingTag { raw } => ParseTaskTagsError::MissingTag { raw },
            TagInputError::InvalidTag { raw } => ParseTaskTagsError::InvalidTag { raw },
        })?;
        Ok(input.0)
    }

    /// Combines parsed tag arguments, returning `None` when no arguments were supplied.
    #[must_use]
    pub fn from_inputs(inputs: &[TagInput]) -> Option<Self> {
        let mut tags = inputs.iter().flat_map(TagInput::iter).cloned();
        let first = tags.next()?;
        Some(Self::from_first_and_rest(first, tags))
    }

    /// Creates a non-empty ordered collection.
    pub fn try_new(values: Vec<Tag>) -> Result<Self, EmptyTaskTagsError> {
        if values.is_empty() {
            return Err(EmptyTaskTagsError);
        }
        Ok(Self(values))
    }

    /// Iterates over tags in first-seen order.
    pub fn iter(&self) -> impl Iterator<Item = &Tag> {
        self.0.iter()
    }

    /// Appends unseen tags while preserving first-seen order.
    #[must_use]
    pub fn merge(&self, appended: &Self) -> Self {
        let mut merged = self.0.clone();
        for tag in appended.iter() {
            push_unseen_tag(&mut merged, tag.clone());
        }
        Self(merged)
    }

    fn from_first_and_rest(first: Tag, tags: impl IntoIterator<Item = Tag>) -> Self {
        let mut unique = vec![first];
        for tag in tags {
            push_unseen_tag(&mut unique, tag);
        }
        Self(unique)
    }
}

fn push_unseen_tag(tags: &mut Vec<Tag>, tag: Tag) {
    if !tags.contains(&tag) {
        tags.push(tag);
    }
}

/// Reports an invalid tag value.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("invalid tag: {raw:?}")]
pub struct InvalidTagError {
    raw: String,
}

/// Reports invalid authored tag syntax.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum TagInputError {
    /// The argument contains an empty tag segment.
    #[error("missing tag value: {raw:?}")]
    MissingTag { raw: String },
    /// A non-empty segment cannot be normalized into a valid tag.
    #[error(
        "invalid tag value {raw:?}; use ASCII letters, digits, '_' or '-', without leading, trailing, or repeated separators"
    )]
    InvalidTag { raw: String },
}

impl TagInputError {
    /// Returns the authored text responsible for the error.
    #[must_use]
    pub fn raw(&self) -> &str {
        match self {
            Self::MissingTag { raw } | Self::InvalidTag { raw } => raw,
        }
    }
}

/// Reports an empty tag collection.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
#[error("tags cannot be empty")]
pub struct EmptyTaskTagsError;

#[must_use]
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum ParseTaskTagsError {
    #[error("missing tag value: {raw:?}")]
    MissingTag { raw: String },
    #[error("invalid tag value: {raw:?}")]
    InvalidTag { raw: String },
    #[error("invalid tags frontmatter: {raw:?}")]
    InvalidFrontmatter { raw: String },
}

impl ParseTaskTagsError {
    #[must_use]
    pub fn raw(&self) -> &str {
        match self {
            Self::MissingTag { raw }
            | Self::InvalidTag { raw }
            | Self::InvalidFrontmatter { raw } => raw,
        }
    }
}

impl IntoIterator for TaskTags {
    type Item = Tag;
    type IntoIter = std::vec::IntoIter<Tag>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::{ParseTaskTagsError, Tag, TagInput, TagInputError, TaskTags};

    fn values(tags: &TaskTags) -> Vec<&str> {
        tags.iter().map(AsRef::as_ref).collect()
    }

    #[test]
    fn tag_and_non_empty_collection_preserve_values() {
        let tags = TaskTags::try_new(vec![
            Tag::try_from("sqlite").unwrap(),
            Tag::try_from("csharp_export").unwrap(),
        ])
        .unwrap();
        assert_eq!(values(&tags), ["sqlite", "csharp_export"]);
        assert!(TaskTags::try_new(Vec::new()).is_err());
    }

    #[test]
    fn authored_inputs_normalize_and_deduplicate_in_encounter_order() {
        let inputs = ["SQLite,csharp-export", "sqlite", " godot ", "csharp_export"]
            .map(|raw| raw.parse::<TagInput>().unwrap());

        let tags = TaskTags::from_inputs(&inputs).unwrap();

        assert_eq!(values(&tags), ["sqlite", "csharp_export", "godot"]);
    }

    #[test]
    fn authored_inputs_reject_empty_segments_and_invalid_characters() {
        for raw in [
            "",
            ",",
            "sqlite,",
            "_sqlite",
            "sqlite_",
            "sqlite__export",
            "c#",
        ] {
            let error = raw.parse::<TagInput>().unwrap_err();
            assert!(matches!(
                error,
                TagInputError::MissingTag { .. } | TagInputError::InvalidTag { .. }
            ));
            assert_eq!(error.raw(), raw);
        }
    }

    #[test]
    fn merge_preserves_first_seen_order() {
        let existing = TaskTags::try_new(
            ["sqlite", "godot"]
                .map(|raw| Tag::try_from(raw).unwrap())
                .to_vec(),
        )
        .unwrap();
        let appended = TaskTags::try_new(
            ["godot", "csharp_export"]
                .map(|raw| Tag::try_from(raw).unwrap())
                .to_vec(),
        )
        .unwrap();

        assert_eq!(
            values(&existing.merge(&appended)),
            ["sqlite", "godot", "csharp_export"]
        );
    }

    #[test]
    fn tag_rejects_invalid_values() {
        for raw in ["SQLite", "csharp-export", " sqlite", "_sqlite", "c#"] {
            assert!(Tag::try_from(raw).is_err(), "input: {raw:?}");
        }
    }

    #[test]
    fn frontmatter_requires_an_inline_array_and_reuses_tag_validation() {
        let tags = TaskTags::parse_frontmatter("[SQLite, csharp-export]").unwrap();
        assert_eq!(
            tags.iter().map(AsRef::as_ref).collect::<Vec<_>>(),
            ["sqlite", "csharp_export"]
        );
        assert!(matches!(
            TaskTags::parse_frontmatter("sqlite"),
            Err(ParseTaskTagsError::InvalidFrontmatter { .. })
        ));
        assert!(TaskTags::parse_frontmatter("[]").is_err());
    }
}
