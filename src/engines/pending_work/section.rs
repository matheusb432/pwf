//! The `Section` a pending-work item is created under within a project note.
//! Replaces stringly-typed section handling on the write path: one place owns the
//! canonical name, the recognized headers (incl. legacy aliases), and the
//! `--section` flag grammar. Absence of a `Section` means the General area (items
//! at the top of the note, no `## ` header).

/// A non-default pending-work section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Future,
    Human,
    LowPrio,
}

impl Section {
    /// Canonical name: the value written to frontmatter and the created
    /// `## {name}` header.
    pub fn as_str(self) -> &'static str {
        match self {
            Section::Future => "Future",
            Section::Human => "Human",
            Section::LowPrio => "Low-prio",
        }
    }

    /// Recognized headers, newest-canonical first; legacy aliases still matched
    /// (`## Futuro`, `## Low-priority`) so existing notes keep parsing (PWF-0026).
    pub fn read_headers(self) -> &'static [&'static str] {
        match self {
            Section::Future => &["## Future", "## Futuro"],
            Section::Human => &["## Human"],
            Section::LowPrio => &["## Low-prio", "## Low-priority"],
        }
    }

    /// Parse the `--section` flag value (`future|human|low-prio`, case-insensitive).
    /// Returns `None` for an unrecognized value so the caller can error with the raw
    /// input.
    pub fn from_flag(value: &str) -> Option<Section> {
        match value.trim().to_lowercase().as_str() {
            "future" => Some(Section::Future),
            "human" => Some(Section::Human),
            "low-prio" => Some(Section::LowPrio),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_names_are_byte_identical() {
        assert_eq!(Section::Future.as_str(), "Future");
        assert_eq!(Section::Human.as_str(), "Human");
        assert_eq!(Section::LowPrio.as_str(), "Low-prio");
    }

    #[test]
    fn read_headers_keep_legacy_aliases_newest_first() {
        assert_eq!(Section::Future.read_headers(), &["## Future", "## Futuro"]);
        assert_eq!(Section::Human.read_headers(), &["## Human"]);
        assert_eq!(
            Section::LowPrio.read_headers(),
            &["## Low-prio", "## Low-priority"]
        );
    }

    #[test]
    fn from_flag_parses_canonical_values_case_insensitively() {
        assert_eq!(Section::from_flag("future"), Some(Section::Future));
        assert_eq!(Section::from_flag("Human"), Some(Section::Human));
        assert_eq!(Section::from_flag("  LOW-PRIO "), Some(Section::LowPrio));
        assert_eq!(Section::from_flag("low"), None);
        assert_eq!(Section::from_flag("bogus"), None);
    }
}
