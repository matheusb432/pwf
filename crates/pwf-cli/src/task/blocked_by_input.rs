use std::{fmt, str::FromStr};

use pwf_models::task::{BlockedBy, TaskId};

/// Contains task IDs parsed from one CLI blocked-by value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct BlockedByInput(Vec<TaskId>);

impl BlockedByInput {
    fn iter(&self) -> impl Iterator<Item = &TaskId> {
        self.0.iter()
    }
}

impl FromStr for BlockedByInput {
    type Err = BlockedByInputError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let mut identifiers = Vec::new();
        for raw in value
            .split(',')
            .map(str::trim)
            .filter(|raw| !raw.is_empty())
        {
            let candidate = raw
                .strip_prefix("[[")
                .and_then(|trimmed| trimmed.strip_suffix("]]"))
                .unwrap_or(raw);
            let identifier =
                candidate
                    .parse::<TaskId>()
                    .map_err(|_| BlockedByInputError::InvalidId {
                        raw: raw.to_string(),
                    })?;
            identifiers.push(identifier);
        }
        if identifiers.is_empty() {
            return Err(BlockedByInputError::MissingId);
        }
        Ok(Self(identifiers))
    }
}

pub(super) fn collect(inputs: &[BlockedByInput]) -> Option<BlockedBy> {
    BlockedBy::try_new(inputs.iter().flat_map(BlockedByInput::iter).cloned()).ok()
}

/// Reports invalid blocked-by CLI syntax.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum BlockedByInputError {
    InvalidId { raw: String },
    MissingId,
}

impl fmt::Display for BlockedByInputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidId { raw } => write!(formatter, "invalid blocked-by task ID: {raw}"),
            Self::MissingId => formatter.write_str("blocked-by input requires a task ID"),
        }
    }
}

impl std::error::Error for BlockedByInputError {}

#[cfg(test)]
mod tests {
    use super::{BlockedByInput, BlockedByInputError, collect};

    #[test]
    fn parses_repeated_compact_comma_separated_and_wikilink_ids() {
        let inputs = [
            "aux57, [[AUX-0014]]".parse::<BlockedByInput>().unwrap(),
            "AUX-14".parse().unwrap(),
        ];

        assert_eq!(
            collect(&inputs)
                .unwrap()
                .iter()
                .map(AsRef::as_ref)
                .collect::<Vec<_>>(),
            ["AUX-0057", "AUX-0014"]
        );
    }

    #[test]
    fn rejects_empty_and_invalid_values() {
        assert!(matches!(
            " , ".parse::<BlockedByInput>(),
            Err(BlockedByInputError::MissingId)
        ));
        assert!(matches!(
            "PWF-99999".parse::<BlockedByInput>(),
            Err(BlockedByInputError::InvalidId { raw }) if raw == "PWF-99999"
        ));
    }
}
