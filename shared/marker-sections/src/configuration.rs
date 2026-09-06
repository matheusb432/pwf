//! Validated runtime configuration for lane markers and rendered headers.

use std::array;

/// Maximum number of Unicode scalar values in one rendered lane header.
pub const LANE_HEADER_CHARACTER_LIMIT: usize = 128;
const MARKER_SLOT_COUNT: usize = 52;
const UNKNOWN_LANE_INDEX: usize = usize::MAX;

/// Defines one lane's input marker and rendered header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaneDefinition {
    marker: String,
    header: String,
}

impl LaneDefinition {
    /// Constructs a lane definition from a `/` plus one ASCII letter marker and a single-line
    /// header.
    ///
    /// # Errors
    ///
    /// Returns [`LaneDefinitionError`] when either value cannot form unambiguous lane syntax.
    pub fn try_new(
        marker: impl Into<String>,
        header: impl Into<String>,
    ) -> Result<Self, LaneDefinitionError> {
        let marker = marker.into();
        validate_marker(&marker)?;
        let header = header.into();
        validate_header(&header)?;
        Ok(Self { marker, header })
    }

    #[must_use]
    pub fn marker(&self) -> &str {
        &self.marker
    }

    #[must_use]
    pub fn header(&self) -> &str {
        &self.header
    }
}

fn validate_marker(marker: &str) -> Result<(), LaneDefinitionError> {
    let bytes = marker.as_bytes();
    if bytes.len() == 2 && bytes[0] == b'/' && bytes[1].is_ascii_alphabetic() {
        return Ok(());
    }
    Err(LaneDefinitionError::InvalidMarker {
        marker: marker.to_string(),
    })
}

fn validate_header(header: &str) -> Result<(), LaneDefinitionError> {
    if header.is_empty() {
        return Err(LaneDefinitionError::EmptyHeader);
    }
    if header.trim() != header {
        return Err(LaneDefinitionError::UntrimmedHeader {
            header: header.to_string(),
        });
    }
    if header.contains(['\n', '\r']) {
        return Err(LaneDefinitionError::MultilineHeader {
            header: header.to_string(),
        });
    }
    let characters = header.chars().count();
    if characters > LANE_HEADER_CHARACTER_LIMIT {
        return Err(LaneDefinitionError::HeaderTooLong {
            characters,
            limit: LANE_HEADER_CHARACTER_LIMIT,
        });
    }
    Ok(())
}

/// Reports one invalid lane definition.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LaneDefinitionError {
    #[error("lane marker must be `/` followed by one ASCII letter: {marker:?}")]
    InvalidMarker { marker: String },
    #[error("lane header cannot be empty")]
    EmptyHeader,
    #[error("lane header cannot start or end with whitespace: {header:?}")]
    UntrimmedHeader { header: String },
    #[error("lane header must be a single line: {header:?}")]
    MultilineHeader { header: String },
    #[error("lane header has {characters} characters; maximum is {limit}")]
    HeaderTooLong { characters: usize, limit: usize },
}

/// Holds an ordered, non-empty set of lane definitions.
///
/// The first lane receives text after `/` and supplies the first Markdown section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaneConfiguration<const N: usize> {
    lanes: [LaneDefinition; N],
    marker_lane_indexes: [usize; MARKER_SLOT_COUNT],
}

impl<const N: usize> LaneConfiguration<N> {
    /// Constructs a configuration with unique markers and headers.
    ///
    /// # Errors
    ///
    /// Returns [`LaneConfigurationError`] when the set is empty or ambiguous.
    pub fn try_new(lanes: [LaneDefinition; N]) -> Result<Self, LaneConfigurationError> {
        if N == 0 {
            return Err(LaneConfigurationError::Empty);
        }
        let marker_lane_indexes = marker_lane_indexes(&lanes)?;
        validate_unique_headers(&lanes)?;
        Ok(Self {
            lanes,
            marker_lane_indexes,
        })
    }

    #[must_use]
    pub fn lanes(&self) -> &[LaneDefinition; N] {
        &self.lanes
    }

    pub(crate) fn empty_lane_items() -> [Vec<String>; N] {
        array::from_fn(|_| Vec::new())
    }

    pub(crate) fn marker_lane(&self, marker_letter: u8) -> MarkerLane {
        let lane_index = self.marker_lane_indexes[marker_slot(marker_letter)];
        if lane_index == UNKNOWN_LANE_INDEX {
            MarkerLane::Unknown
        } else {
            MarkerLane::Configured(lane_index)
        }
    }
}

pub(crate) enum MarkerLane {
    Configured(usize),
    Unknown,
}

fn marker_lane_indexes(
    lanes: &[LaneDefinition],
) -> Result<[usize; MARKER_SLOT_COUNT], LaneConfigurationError> {
    let mut indexes = [UNKNOWN_LANE_INDEX; MARKER_SLOT_COUNT];
    for (lane_index, lane) in lanes.iter().enumerate() {
        let slot = marker_slot(lane.marker.as_bytes()[1]);
        if indexes[slot] != UNKNOWN_LANE_INDEX {
            return Err(LaneConfigurationError::DuplicateMarker {
                marker: lane.marker.clone(),
            });
        }
        indexes[slot] = lane_index;
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

fn validate_unique_headers(lanes: &[LaneDefinition]) -> Result<(), LaneConfigurationError> {
    for (index, lane) in lanes.iter().enumerate() {
        if lanes[index + 1..]
            .iter()
            .any(|other| lane.header == other.header)
        {
            return Err(LaneConfigurationError::DuplicateHeader {
                header: lane.header.clone(),
            });
        }
    }
    Ok(())
}

/// Reports an ambiguous or empty lane configuration.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LaneConfigurationError {
    #[error("lane configuration cannot be empty")]
    Empty,
    #[error("lane marker is configured more than once: {marker:?}")]
    DuplicateMarker { marker: String },
    #[error("lane header is configured more than once: {header:?}")]
    DuplicateHeader { header: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lane_definition_rejects_ambiguous_values() {
        assert!(matches!(
            LaneDefinition::try_new("goal", "Goals"),
            Err(LaneDefinitionError::InvalidMarker { .. })
        ));
        assert!(matches!(
            LaneDefinition::try_new("/g", " Goals"),
            Err(LaneDefinitionError::UntrimmedHeader { .. })
        ));
        assert!(matches!(
            LaneDefinition::try_new("/g", "Goals\nLater"),
            Err(LaneDefinitionError::MultilineHeader { .. })
        ));
    }

    #[test]
    fn configuration_rejects_duplicate_markers_and_headers() {
        let duplicate_marker = [
            LaneDefinition::try_new("/g", "Goals").unwrap(),
            LaneDefinition::try_new("/g", "Context").unwrap(),
        ];
        assert!(matches!(
            LaneConfiguration::try_new(duplicate_marker),
            Err(LaneConfigurationError::DuplicateMarker { .. })
        ));

        let duplicate_header = [
            LaneDefinition::try_new("/g", "Goals").unwrap(),
            LaneDefinition::try_new("/c", "Goals").unwrap(),
        ];
        assert!(matches!(
            LaneConfiguration::try_new(duplicate_header),
            Err(LaneConfigurationError::DuplicateHeader { .. })
        ));
    }
}
