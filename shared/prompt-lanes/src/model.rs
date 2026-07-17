use crate::title::cap_title;

/// Contains a parsed title and four ordered bullet sections.
///
/// `title` is uncapped and case-preserved. A non-empty title also becomes the first Goals bullet;
/// marker-first prompts leave both empty.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ParsedPrompt {
    pub title: String,
    pub goals: Vec<String>,
    pub context: Vec<String>,
    pub constraints: Vec<String>,
    pub done_when: Vec<String>,
}

impl ParsedPrompt {
    /// Returns [`Self::title`] capped at a word boundary, with an ellipsis when truncated.
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
