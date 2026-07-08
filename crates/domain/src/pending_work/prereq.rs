use thiserror::Error;

use crate::pending_work::WorkItemId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prereqs {
    ids: Vec<WorkItemId>,
}

impl Prereqs {
    pub fn parse_values(values: &[String]) -> Result<Self, ParsePrereqsError> {
        let mut ids = Vec::new();
        for value in values {
            for raw in value.split(',') {
                let raw = raw.trim();
                if raw.is_empty() {
                    continue;
                }
                let id = raw
                    .strip_prefix("[[")
                    .and_then(|trimmed| trimmed.strip_suffix("]]"))
                    .unwrap_or(raw);
                let id = WorkItemId::try_new(id).map_err(|_| ParsePrereqsError::InvalidId {
                    raw: raw.to_string(),
                })?;
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
        }
        if ids.is_empty() {
            return Err(ParsePrereqsError::MissingId);
        }
        Ok(Self { ids })
    }

    pub fn ids(&self) -> Vec<&str> {
        self.ids.iter().map(AsRef::as_ref).collect()
    }

    pub fn iter(&self) -> impl Iterator<Item = &WorkItemId> {
        self.ids.iter()
    }

    pub fn frontmatter_value(&self) -> String {
        self.ids
            .iter()
            .map(|id| format!("[[{id}]]"))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ParsePrereqsError {
    #[error("missing prereq id")]
    MissingId,
    #[error("invalid prereq id: {raw}")]
    InvalidId { raw: String },
}

#[cfg(test)]
mod tests {
    use super::Prereqs;

    #[test]
    fn parse_normalizes_and_deduplicates_ids() {
        let prereqs =
            Prereqs::parse_values(&["cfg57, [[CFG-0014]]".to_string(), "CFG-14".to_string()])
                .unwrap();

        assert_eq!(prereqs.ids(), ["CFG-0057", "CFG-0014"]);
        assert_eq!(prereqs.frontmatter_value(), "[[CFG-0057]], [[CFG-0014]]");
    }

    #[test]
    fn parse_rejects_empty_values() {
        assert!(Prereqs::parse_values(&[]).is_err());
        assert!(Prereqs::parse_values(&[" , ".to_string()]).is_err());
    }
}
