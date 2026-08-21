use pwf_models::task::{TagInput, TagInputError, TaskTags};

use crate::contract::task::RawTaskTags;

pub(in crate::task) fn parse_frontmatter(raw: &RawTaskTags) -> Result<TaskTags, ParseTagsError> {
    let raw = raw.as_ref();
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
    TaskTags::from_inputs(&[input]).ok_or_else(|| ParseTagsError::MissingTag {
        raw: raw.to_string(),
    })
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
    use pwf_models::task::TaskTags;

    use super::{ParseTagsError, parse_frontmatter};
    use crate::contract::task::RawTaskTags;

    fn values(tags: &TaskTags) -> Vec<&str> {
        tags.iter().map(AsRef::as_ref).collect()
    }

    fn raw_tags(raw: &str) -> RawTaskTags {
        RawTaskTags::new(raw)
    }

    #[test]
    fn frontmatter_requires_an_inline_array_and_reuses_tag_validation() {
        let tags = parse_frontmatter(&raw_tags("[SQLite, csharp-export]")).unwrap();
        assert_eq!(values(&tags), ["sqlite", "csharp_export"]);

        assert!(matches!(
            parse_frontmatter(&raw_tags("sqlite")),
            Err(ParseTagsError::InvalidFrontmatter { .. })
        ));
        assert!(parse_frontmatter(&raw_tags("[]")).is_err());
    }
}
