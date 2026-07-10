use std::fmt;

use thiserror::Error;

/// Stores one normalized lowercase snake-case task label.
///
/// # Examples
///
/// ```
/// use pwf_domain::pending_work::Tag;
///
/// let tag = Tag::try_from("CSharp-Export").unwrap();
/// assert_eq!(tag.as_ref(), "csharp_export");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Tag(String);

impl TryFrom<&str> for Tag {
    type Error = ParseTagsError;

    fn try_from(raw: &str) -> Result<Self, Self::Error> {
        let normalized = raw.trim().to_ascii_lowercase().replace('-', "_");
        let valid_chars = normalized
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_');
        let valid_separators = !normalized.starts_with('_')
            && !normalized.ends_with('_')
            && !normalized.contains("__");
        let has_alphanumeric = normalized.chars().any(|ch| ch.is_ascii_alphanumeric());
        if normalized.is_empty() || !valid_chars || !valid_separators || !has_alphanumeric {
            return Err(ParseTagsError::InvalidTag {
                raw: raw.to_string(),
            });
        }
        Ok(Self(normalized))
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

/// Stores ordered, deduplicated task labels.
///
/// # Examples
///
/// ```
/// use pwf_domain::pending_work::Tags;
///
/// let tags = Tags::parse_values(&["SQLite,csharp-export".to_string()]).unwrap();
/// assert_eq!(tags.frontmatter_value(), "[sqlite, csharp_export]");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tags(Vec<Tag>);

impl Tags {
    /// Parses repeatable and comma-separated values in encounter order.
    ///
    /// # Errors
    ///
    /// Returns [`ParseTagsError`] when no tag is supplied or any segment is invalid.
    ///
    /// # Examples
    ///
    /// ```
    /// use pwf_domain::pending_work::Tags;
    ///
    /// let tags = Tags::parse_values(&["godot,setup".to_string()]).unwrap();
    /// assert_eq!(tags.iter().count(), 2);
    /// ```
    pub fn parse_values(values: &[String]) -> Result<Self, ParseTagsError> {
        if values.is_empty() {
            return Err(ParseTagsError::MissingTag { raw: String::new() });
        }
        let mut tags = Vec::new();
        for value in values {
            let segments: Vec<&str> = value.split(',').collect();
            if segments.iter().any(|raw| raw.trim().is_empty()) {
                return Err(ParseTagsError::MissingTag { raw: value.clone() });
            }
            for raw in segments {
                let tag = Tag::try_from(raw).map_err(|_| ParseTagsError::InvalidTag {
                    raw: raw.to_string(),
                })?;
                if !tags.contains(&tag) {
                    tags.push(tag);
                }
            }
        }
        Ok(Self(tags))
    }

    /// Parses the payload of a `tags:` inline frontmatter array.
    ///
    /// # Errors
    ///
    /// Returns [`ParseTagsError`] when the value is not a non-empty inline array of valid tags.
    ///
    /// # Examples
    ///
    /// ```
    /// use pwf_domain::pending_work::Tags;
    ///
    /// let tags = Tags::parse_frontmatter("[godot, csharp-export]").unwrap();
    /// assert_eq!(tags.frontmatter_value(), "[godot, csharp_export]");
    /// ```
    pub fn parse_frontmatter(raw: &str) -> Result<Self, ParseTagsError> {
        let trimmed = raw.trim();
        let Some(inner) = trimmed
            .strip_prefix('[')
            .and_then(|value| value.strip_suffix(']'))
        else {
            return Err(ParseTagsError::InvalidFrontmatter {
                raw: raw.to_string(),
            });
        };
        if inner.trim().is_empty() {
            return Err(ParseTagsError::MissingTag {
                raw: raw.to_string(),
            });
        }
        Self::parse_values(&[inner.to_string()])
    }

    #[must_use]
    /// Returns the first-seen union of existing and appended tags.
    ///
    /// # Examples
    ///
    /// ```
    /// use pwf_domain::pending_work::Tags;
    ///
    /// let left = Tags::parse_frontmatter("[godot]").unwrap();
    /// let right = Tags::parse_frontmatter("[godot, setup]").unwrap();
    /// assert_eq!(left.merged(&right).frontmatter_value(), "[godot, setup]");
    /// ```
    pub fn merged(&self, appended: &Self) -> Self {
        let mut merged = self.0.clone();
        for tag in &appended.0 {
            if !merged.contains(tag) {
                merged.push(tag.clone());
            }
        }
        Self(merged)
    }

    /// Reports whether every requested tag is present.
    ///
    /// # Examples
    ///
    /// ```
    /// use pwf_domain::pending_work::Tags;
    ///
    /// let item = Tags::parse_frontmatter("[godot, setup]").unwrap();
    /// let wanted = Tags::parse_frontmatter("[setup]").unwrap();
    /// assert!(item.contains_all(&wanted));
    /// ```
    pub fn contains_all(&self, requested: &Self) -> bool {
        requested.0.iter().all(|tag| self.0.contains(tag))
    }

    /// Renders the canonical inline-array frontmatter payload.
    ///
    /// # Examples
    ///
    /// ```
    /// use pwf_domain::pending_work::Tags;
    ///
    /// let tags = Tags::parse_frontmatter("[Godot, csharp-export]").unwrap();
    /// assert_eq!(tags.frontmatter_value(), "[godot, csharp_export]");
    /// ```
    pub fn frontmatter_value(&self) -> String {
        format!(
            "[{}]",
            self.0
                .iter()
                .map(Tag::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        )
    }

    /// Iterates over tags in first-seen order.
    ///
    /// # Examples
    ///
    /// ```
    /// use pwf_domain::pending_work::Tags;
    ///
    /// let tags = Tags::parse_frontmatter("[godot, setup]").unwrap();
    /// assert_eq!(
    ///     tags.iter().map(AsRef::as_ref).collect::<Vec<_>>(),
    ///     ["godot", "setup"]
    /// );
    /// ```
    pub fn iter(&self) -> impl Iterator<Item = &Tag> {
        self.0.iter()
    }
}

/// Describes invalid CLI or frontmatter tag text while retaining the offending value.
///
/// # Examples
///
/// ```
/// use pwf_domain::pending_work::{ParseTagsError, Tags};
///
/// let error = Tags::parse_values(&["bad tag".to_string()]).unwrap_err();
/// assert!(matches!(error, ParseTagsError::InvalidTag { .. }));
/// ```
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ParseTagsError {
    #[error("missing tag value: {raw:?}")]
    MissingTag { raw: String },
    #[error("invalid tag value: {raw:?}")]
    InvalidTag { raw: String },
    #[error("invalid tags frontmatter: {raw:?}")]
    InvalidFrontmatter { raw: String },
}

impl ParseTagsError {
    /// Returns the raw value that failed validation.
    ///
    /// # Examples
    ///
    /// ```
    /// use pwf_domain::pending_work::Tags;
    ///
    /// let error = Tags::parse_values(&["bad tag".to_string()]).unwrap_err();
    /// assert_eq!(error.raw(), "bad tag");
    /// ```
    pub fn raw(&self) -> &str {
        match self {
            Self::MissingTag { raw }
            | Self::InvalidTag { raw }
            | Self::InvalidFrontmatter { raw } => raw,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ParseTagsError, Tags};

    fn values(tags: &Tags) -> Vec<&str> {
        tags.iter().map(AsRef::as_ref).collect()
    }

    #[test]
    fn parse_values_normalizes_and_deduplicates_in_encounter_order() {
        let tags = Tags::parse_values(&[
            "SQLite,csharp-export".to_string(),
            "sqlite".to_string(),
            " godot ".to_string(),
            "csharp_export".to_string(),
        ])
        .unwrap();

        assert_eq!(values(&tags), ["sqlite", "csharp_export", "godot"]);
        assert_eq!(tags.frontmatter_value(), "[sqlite, csharp_export, godot]");
    }

    #[test]
    fn parse_values_rejects_empty_segments_and_invalid_characters() {
        for raw in [
            "",
            ",",
            "sqlite,",
            "_sqlite",
            "sqlite_",
            "sqlite__export",
            "c#",
        ] {
            let error = Tags::parse_values(&[raw.to_string()]).unwrap_err();
            assert_eq!(error.raw(), raw);
        }
    }

    #[test]
    fn parse_frontmatter_requires_an_inline_array_and_reuses_tag_validation() {
        let tags = Tags::parse_frontmatter("[SQLite, csharp-export]").unwrap();
        assert_eq!(values(&tags), ["sqlite", "csharp_export"]);

        assert!(matches!(
            Tags::parse_frontmatter("sqlite"),
            Err(ParseTagsError::InvalidFrontmatter { .. })
        ));
        assert!(Tags::parse_frontmatter("[]").is_err());
    }

    #[test]
    fn merge_and_contains_all_preserve_set_semantics() {
        let existing = Tags::parse_frontmatter("[sqlite, godot]").unwrap();
        let appended = Tags::parse_values(&["godot,csharp-export".to_string()]).unwrap();
        let merged = existing.merged(&appended);

        assert_eq!(values(&merged), ["sqlite", "godot", "csharp_export"]);
        assert!(merged.contains_all(&appended));
        assert!(!appended.contains_all(&existing));
    }
}
