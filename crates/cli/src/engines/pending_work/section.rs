//! Defines canonical sections, accepted flag values, and legacy header aliases.
//! `None` represents the unheaded general section.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Future,
    Human,
    LowPrio,
}

impl Section {
    /// Returns the canonical frontmatter and header name.
    pub fn as_str(self) -> &'static str {
        match self {
            Section::Future => "Future",
            Section::Human => "Human",
            Section::LowPrio => "Low-prio",
        }
    }

    /// Returns canonical and legacy headers, with the canonical form first.
    pub fn read_headers(self) -> &'static [&'static str] {
        match self {
            Section::Future => &["## Future", "## Futuro"],
            Section::Human => &["## Human"],
            Section::LowPrio => &["## Low-prio", "## Low-priority"],
        }
    }

    /// Parses a canonical CLI value case-insensitively.
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
