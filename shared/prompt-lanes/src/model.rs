use crate::title::cap_title;

/// A prompt parsed into a title plus four named, ordered bullet sections.
///
/// `title` is the raw lead clause (uncapped, case-preserved). When present, it
/// is also the first Goals bullet; marker-first prompts leave both empty.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ParsedPrompt {
    pub title: String,
    pub goals: Vec<String>,
    pub context: Vec<String>,
    pub constraints: Vec<String>,
    pub done_when: Vec<String>,
}

impl ParsedPrompt {
    /// Returns [`Self::title`] capped at `max_chars`, word-boundary-safe with
    /// an ellipsis when truncated.
    pub fn capped_title(&self, max_chars: usize) -> String {
        cap_title(&self.title, max_chars)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capped_title_delegates_to_cap_title_with_caller_supplied_max() {
        let parsed = ParsedPrompt {
            title: "word ".repeat(20).trim().to_string(),
            ..ParsedPrompt::default()
        };
        let capped = parsed.capped_title(10);
        assert!(capped.chars().count() <= 11);
        assert!(capped.ends_with('…'));
    }

    #[test]
    fn capped_title_returns_short_title_unchanged() {
        let parsed = ParsedPrompt {
            title: "short title".to_string(),
            ..ParsedPrompt::default()
        };
        assert_eq!(parsed.capped_title(80), "short title");
    }
}
