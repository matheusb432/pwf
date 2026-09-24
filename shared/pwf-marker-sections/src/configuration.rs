//! Validated runtime configuration for section markers, rendered headers, and item layout.

/// Maximum number of Unicode scalar values in one rendered section header.
pub const MARKER_SECTION_HEADER_CHARACTER_LIMIT: usize = 128;
const MARKER_SLOT_COUNT: usize = 52;
const UNKNOWN_SECTION_INDEX: usize = usize::MAX;

/// Holds a Markdown ATX heading level from 1 through 6.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarkerSectionHeadingLevel(u8);

impl MarkerSectionHeadingLevel {
    /// Constructs a heading level.
    ///
    /// # Errors
    ///
    /// Returns [`MarkerSectionHeadingLevelError`] outside `1..=6`.
    pub const fn try_new(level: u8) -> Result<Self, MarkerSectionHeadingLevelError> {
        if matches!(level, 1..=6) {
            Ok(Self(level))
        } else {
            Err(MarkerSectionHeadingLevelError { level })
        }
    }

    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("section heading level must be from 1 through 6; found {level}")]
pub struct MarkerSectionHeadingLevelError {
    level: u8,
}

/// Selects how a rendered section separates its items.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkerSectionItemStyle {
    /// One `- ` list item per line.
    Bullet,
    /// One `1. ` list item per line, numbered in order.
    Numbered,
    /// One paragraph per item, separated by a blank line.
    Paragraph,
}

/// Defines one section's input marker, rendered header, heading level, and item style.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkerSectionDefinition {
    marker: String,
    header: String,
    heading_level: MarkerSectionHeadingLevel,
    item_style: MarkerSectionItemStyle,
}

impl MarkerSectionDefinition {
    /// Constructs a section definition from a `/` plus one ASCII letter marker and a single-line
    /// header.
    ///
    /// # Errors
    ///
    /// Returns [`MarkerSectionDefinitionError`] when either value cannot form unambiguous section
    /// syntax.
    pub fn try_new(
        marker: impl Into<String>,
        header: impl Into<String>,
        heading_level: MarkerSectionHeadingLevel,
        item_style: MarkerSectionItemStyle,
    ) -> Result<Self, MarkerSectionDefinitionError> {
        let marker = marker.into();
        validate_marker(&marker)?;
        let header = header.into();
        validate_header(&header)?;
        Ok(Self {
            marker,
            header,
            heading_level,
            item_style,
        })
    }

    #[must_use]
    pub fn marker(&self) -> &str {
        &self.marker
    }

    #[must_use]
    pub fn header(&self) -> &str {
        &self.header
    }

    #[must_use]
    pub const fn heading_level(&self) -> MarkerSectionHeadingLevel {
        self.heading_level
    }

    #[must_use]
    pub const fn item_style(&self) -> MarkerSectionItemStyle {
        self.item_style
    }
}

fn validate_marker(marker: &str) -> Result<(), MarkerSectionDefinitionError> {
    let bytes = marker.as_bytes();
    if bytes.len() == 2 && bytes[0] == b'/' && bytes[1].is_ascii_alphabetic() {
        return Ok(());
    }
    Err(MarkerSectionDefinitionError::InvalidMarker {
        marker: marker.to_string(),
    })
}

fn validate_header(header: &str) -> Result<(), MarkerSectionDefinitionError> {
    if header.is_empty() {
        return Err(MarkerSectionDefinitionError::EmptyHeader);
    }
    if header.trim() != header {
        return Err(MarkerSectionDefinitionError::UntrimmedHeader {
            header: header.to_string(),
        });
    }
    if header.contains(['\n', '\r']) {
        return Err(MarkerSectionDefinitionError::MultilineHeader {
            header: header.to_string(),
        });
    }
    let characters = header.chars().count();
    if characters > MARKER_SECTION_HEADER_CHARACTER_LIMIT {
        return Err(MarkerSectionDefinitionError::HeaderTooLong {
            characters,
            limit: MARKER_SECTION_HEADER_CHARACTER_LIMIT,
        });
    }
    Ok(())
}

/// Reports one invalid section definition.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MarkerSectionDefinitionError {
    #[error("section marker must be `/` followed by one ASCII letter: {marker:?}")]
    InvalidMarker { marker: String },
    #[error("section header cannot be empty")]
    EmptyHeader,
    #[error("section header cannot start or end with whitespace: {header:?}")]
    UntrimmedHeader { header: String },
    #[error("section header must be a single line: {header:?}")]
    MultilineHeader { header: String },
    #[error("section header has {characters} characters; maximum is {limit}")]
    HeaderTooLong { characters: usize, limit: usize },
}

/// Holds an ordered, non-empty set of section definitions.
///
/// The first section receives text after `/` and supplies the first Markdown section. Unique
/// markers bound the set to 52 sections.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkerSectionConfiguration {
    sections: Box<[MarkerSectionDefinition]>,
    marker_section_indexes: [usize; MARKER_SLOT_COUNT],
}

impl MarkerSectionConfiguration {
    /// Constructs a configuration with unique markers and headers.
    ///
    /// # Errors
    ///
    /// Returns [`MarkerSectionConfigurationError`] when the set is empty or ambiguous.
    pub fn try_new(
        sections: Vec<MarkerSectionDefinition>,
    ) -> Result<Self, MarkerSectionConfigurationError> {
        if sections.is_empty() {
            return Err(MarkerSectionConfigurationError::Empty);
        }
        let marker_section_indexes = marker_section_indexes(&sections)?;
        validate_unique_headers(&sections)?;
        Ok(Self {
            sections: sections.into_boxed_slice(),
            marker_section_indexes,
        })
    }

    #[must_use]
    pub fn sections(&self) -> &[MarkerSectionDefinition] {
        &self.sections
    }

    pub(crate) fn marker_section(&self, marker_letter: u8) -> MarkerSection {
        let section_index = self.marker_section_indexes[marker_slot(marker_letter)];
        if section_index == UNKNOWN_SECTION_INDEX {
            MarkerSection::Unknown
        } else {
            MarkerSection::Configured(section_index)
        }
    }
}

pub(crate) enum MarkerSection {
    Configured(usize),
    Unknown,
}

fn marker_section_indexes(
    sections: &[MarkerSectionDefinition],
) -> Result<[usize; MARKER_SLOT_COUNT], MarkerSectionConfigurationError> {
    let mut indexes = [UNKNOWN_SECTION_INDEX; MARKER_SLOT_COUNT];
    for (section_index, section) in sections.iter().enumerate() {
        let slot = marker_slot(section.marker.as_bytes()[1]);
        if indexes[slot] != UNKNOWN_SECTION_INDEX {
            return Err(MarkerSectionConfigurationError::DuplicateMarker {
                marker: section.marker.clone(),
            });
        }
        indexes[slot] = section_index;
    }
    Ok(indexes)
}

const fn marker_slot(marker_letter: u8) -> usize {
    if marker_letter.is_ascii_uppercase() {
        (marker_letter - b'A') as usize
    } else {
        26 + (marker_letter - b'a') as usize
    }
}

fn validate_unique_headers(
    sections: &[MarkerSectionDefinition],
) -> Result<(), MarkerSectionConfigurationError> {
    for (index, section) in sections.iter().enumerate() {
        if sections[index + 1..]
            .iter()
            .any(|other| section.header == other.header)
        {
            return Err(MarkerSectionConfigurationError::DuplicateHeader {
                header: section.header.clone(),
            });
        }
    }
    Ok(())
}

/// Reports an ambiguous or empty section configuration.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MarkerSectionConfigurationError {
    #[error("section configuration cannot be empty")]
    Empty,
    #[error("section marker is configured more than once: {marker:?}")]
    DuplicateMarker { marker: String },
    #[error("section header is configured more than once: {header:?}")]
    DuplicateHeader { header: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn definition(
        marker: &str,
        header: &str,
    ) -> Result<MarkerSectionDefinition, MarkerSectionDefinitionError> {
        MarkerSectionDefinition::try_new(
            marker,
            header,
            MarkerSectionHeadingLevel::try_new(2).unwrap(),
            MarkerSectionItemStyle::Bullet,
        )
    }

    #[test]
    fn marker_section_definition_rejects_ambiguous_values() {
        assert!(matches!(
            definition("goal", "Goals"),
            Err(MarkerSectionDefinitionError::InvalidMarker { .. })
        ));
        assert!(matches!(
            definition("/g", " Goals"),
            Err(MarkerSectionDefinitionError::UntrimmedHeader { .. })
        ));
        assert!(matches!(
            definition("/g", "Goals\nLater"),
            Err(MarkerSectionDefinitionError::MultilineHeader { .. })
        ));
    }

    #[test]
    fn heading_level_accepts_only_markdown_atx_levels() {
        for level in 1..=6 {
            assert_eq!(
                MarkerSectionHeadingLevel::try_new(level).unwrap().get(),
                level
            );
        }
        for level in [0, 7, u8::MAX] {
            assert!(MarkerSectionHeadingLevel::try_new(level).is_err());
        }
    }

    #[test]
    fn configuration_rejects_empty_and_duplicate_markers_and_headers() {
        assert!(matches!(
            MarkerSectionConfiguration::try_new(Vec::new()),
            Err(MarkerSectionConfigurationError::Empty)
        ));

        let duplicate_marker = vec![
            definition("/g", "Goals").unwrap(),
            definition("/g", "Context").unwrap(),
        ];
        assert!(matches!(
            MarkerSectionConfiguration::try_new(duplicate_marker),
            Err(MarkerSectionConfigurationError::DuplicateMarker { .. })
        ));

        let duplicate_header = vec![
            definition("/g", "Goals").unwrap(),
            definition("/c", "Goals").unwrap(),
        ];
        assert!(matches!(
            MarkerSectionConfiguration::try_new(duplicate_header),
            Err(MarkerSectionConfigurationError::DuplicateHeader { .. })
        ));
    }
}
