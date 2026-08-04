use pwf_models::task::{Tag, Tags};

pub(in crate::task) fn parse_values(values: &[String]) -> Result<Tags, ParseTagsError> {
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
            let normalized = raw.trim().to_ascii_lowercase().replace('-', "_");
            let tag =
                Tag::try_from(normalized.as_str()).map_err(|_| ParseTagsError::InvalidTag {
                    raw: raw.to_string(),
                })?;
            if !tags.contains(&tag) {
                tags.push(tag);
            }
        }
    }
    Tags::try_new(tags).map_err(|_| ParseTagsError::MissingTag { raw: String::new() })
}

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
    parse_values(&[inner.to_string()])
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

    use super::{ParseTagsError, contains_all, merge, parse_frontmatter, parse_values};

    fn values(tags: &Tags) -> Vec<&str> {
        tags.iter().map(AsRef::as_ref).collect()
    }

    #[test]
    fn cli_values_normalize_and_deduplicate_in_encounter_order() {
        let tags = parse_values(&[
            "SQLite,csharp-export".to_string(),
            "sqlite".to_string(),
            " godot ".to_string(),
            "csharp_export".to_string(),
        ])
        .unwrap();

        assert_eq!(values(&tags), ["sqlite", "csharp_export", "godot"]);
    }

    #[test]
    fn cli_values_reject_empty_segments_and_invalid_characters() {
        for raw in [
            "",
            ",",
            "sqlite,",
            "_sqlite",
            "sqlite_",
            "sqlite__export",
            "c#",
        ] {
            let error = parse_values(&[raw.to_string()]).unwrap_err();
            assert_eq!(error.raw(), raw);
        }
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
        let appended = parse_values(&["godot,csharp-export".to_string()]).unwrap();
        let merged = merge(&existing, &appended);

        assert_eq!(values(&merged), ["sqlite", "godot", "csharp_export"]);
        assert!(contains_all(&merged, &appended));
        assert!(!contains_all(&appended, &existing));
    }
}
