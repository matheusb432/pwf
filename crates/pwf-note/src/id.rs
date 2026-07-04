//! `NoteId` — parse/normalize a note id for a project-scoped command.
//!
//! Because `pwf note <proj> …` already names the project, three input forms all
//! resolve to the canonical `{PREFIX}-NOTE-{NNNN}`: the full id, the prefix-less
//! `NOTE-NNNN`, and a bare number.

use crate::errors::NoteError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteId {
    pub canonical: String,
    pub number: u32,
}

impl NoteId {
    /// Resolve `input` against the project's `prefix` (e.g. `"PWF"`).
    pub fn resolve(input: &str, prefix: &str) -> Result<NoteId, NoteError> {
        // Normalize the prefix so a config-sourced lowercase prefix still yields
        // a canonical (uppercase) id and matches the uppercased input.
        let prefix = prefix.trim().to_uppercase();
        let up = input.trim().to_uppercase();
        let full = format!("{prefix}-NOTE-");
        let digits = up
            .strip_prefix(&full)
            .or_else(|| up.strip_prefix("NOTE-"))
            .unwrap_or(&up);
        let number: u32 = digits
            .parse()
            .map_err(|_| NoteError::BadId(input.to_string(), prefix.clone()))?;
        Ok(NoteId {
            canonical: format!("{prefix}-NOTE-{number:04}"),
            number,
        })
    }

    /// The note's `.md` file name (`{canonical}.md`).
    pub fn file_name(&self) -> String {
        format!("{}.md", self.canonical)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_id_normalizes_case() {
        let id = NoteId::resolve("pwf-note-0003", "PWF").unwrap();
        assert_eq!(id.canonical, "PWF-NOTE-0003");
        assert_eq!(id.number, 3);
    }

    #[test]
    fn prefixless_form_reattaches_prefix() {
        assert_eq!(
            NoteId::resolve("NOTE-0007", "PWF").unwrap().canonical,
            "PWF-NOTE-0007"
        );
    }

    #[test]
    fn lowercase_prefix_yields_canonical_uppercase() {
        let id = NoteId::resolve("pwf-note-0003", "pwf").unwrap();
        assert_eq!(id.canonical, "PWF-NOTE-0003");
        assert_eq!(
            NoteId::resolve("5", "pwf").unwrap().canonical,
            "PWF-NOTE-0005"
        );
    }

    #[test]
    fn bare_number_zero_pads() {
        assert_eq!(
            NoteId::resolve("3", "PWF").unwrap().canonical,
            "PWF-NOTE-0003"
        );
        assert_eq!(
            NoteId::resolve("42", "PWF").unwrap().canonical,
            "PWF-NOTE-0042"
        );
    }

    #[test]
    fn garbage_errors() {
        assert!(matches!(
            NoteId::resolve("xyz", "PWF"),
            Err(NoteError::BadId(..))
        ));
        // A different project's full id is not valid here.
        assert!(matches!(
            NoteId::resolve("GLP-NOTE-0001", "PWF"),
            Err(NoteError::BadId(..))
        ));
    }
}
