use pwf_models::task::{TagInput, TagInputError, Tags};

pub(in crate::task) fn parse_frontmatter(raw: &str) -> Result<Tags, ParseTagsError> {
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
    let input = inner.parse::<TagInput>().map_err(|error| match error {
        TagInputError::MissingTag { raw } => ParseTagsError::MissingTag { raw },
        TagInputError::InvalidTag { raw } => ParseTagsError::InvalidTag { raw },
    })?;
    Tags::from_inputs(&[input]).ok_or_else(|| ParseTagsError::MissingTag {
        raw: raw.to_string(),
    })
}

#[must_use]
pub(in crate::task) fn merge(existing: &Tags, appended: &Tags) -> Tags {
    existing.merge(appended)
}

#[must_use]
pub(in crate::task) fn contains_all(stored: &Tags, requested: &Tags) -> bool {
    requested
        .iter()
        .all(|requested| stored.iter().any(|stored| stored == requested))
}

#[must_use]
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub(in crate::task) enum ParseTagsError {
    #[error("missing tag value: {raw:?}")]
    MissingTag { raw: String },
    #[error("invalid tag value: {raw:?}")]
    InvalidTag { raw: String },
    #[error("invalid tags frontmatter: {raw:?}")]
    InvalidFrontmatter { raw: String },
}

impl ParseTagsError {
    pub(in crate::task) fn raw(&self) -> &str {
        match self {
            Self::MissingTag { raw }
            | Self::InvalidTag { raw }
            | Self::InvalidFrontmatter { raw } => raw,
        }
    }
}

#[cfg(test)]
mod tests {
    use pwf_models::task::Tags;

    use super::{ParseTagsError, contains_all, merge, parse_frontmatter};

    fn values(tags: &Tags) -> Vec<&str> {
        tags.iter().map(AsRef::as_ref).collect()
    }

    #[test]
    fn frontmatter_requires_an_inline_array_and_reuses_tag_validation() {
        let tags = parse_frontmatter("[SQLite, csharp-export]").unwrap();
        assert_eq!(values(&tags), ["sqlite", "csharp_export"]);

        assert!(matches!(
            parse_frontmatter("sqlite"),
            Err(ParseTagsError::InvalidFrontmatter { .. })
        ));
        assert!(parse_frontmatter("[]").is_err());
    }

    #[test]
    fn merge_and_inclusion_preserve_set_semantics() {
        let existing = parse_frontmatter("[sqlite, godot]").unwrap();
        let appended = Tags::from_inputs(&["godot,csharp-export".parse().unwrap()]).unwrap();
        let merged = merge(&existing, &appended);

        assert_eq!(values(&merged), ["sqlite", "godot", "csharp_export"]);
        assert!(contains_all(&merged, &appended));
        assert!(!contains_all(&appended, &existing));
    }
}
