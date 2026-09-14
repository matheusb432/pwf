use std::array;

/// Contains a parsed title and one ordered item collection per configured section.
#[derive(Debug, PartialEq, Eq)]
pub struct ParsedMarkerSections<const N: usize> {
    pub(crate) title: String,
    pub(crate) section_items: [Vec<String>; N],
}

impl<const N: usize> ParsedMarkerSections<N> {
    #[must_use]
    pub fn new(title: String, section_items: [Vec<String>; N]) -> Self {
        Self {
            title,
            section_items,
        }
    }

    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    #[must_use]
    pub fn section_items(&self) -> &[Vec<String>; N] {
        &self.section_items
    }

    #[must_use]
    pub fn into_parts(self) -> (String, [Vec<String>; N]) {
        (self.title, self.section_items)
    }
}

impl<const N: usize> Default for ParsedMarkerSections<N> {
    fn default() -> Self {
        Self {
            title: String::new(),
            section_items: array::from_fn(|_| Vec::new()),
        }
    }
}
