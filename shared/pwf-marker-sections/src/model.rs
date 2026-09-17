use std::array;

/// Contains a parsed title, marker presence, and ordered items for each configured section.
#[derive(Debug, PartialEq, Eq)]
pub struct ParsedMarkerSections<const N: usize> {
    pub(crate) title: String,
    pub(crate) section_items: [Vec<String>; N],
    pub(crate) section_presence: [bool; N],
}

impl<const N: usize> ParsedMarkerSections<N> {
    /// Creates parsed content and treats every section with items as explicitly present.
    #[must_use]
    pub fn new(title: String, section_items: [Vec<String>; N]) -> Self {
        let section_presence = array::from_fn(|index| !section_items[index].is_empty());
        Self {
            title,
            section_items,
            section_presence,
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

    /// Returns the title and items without the original marker-presence metadata.
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
            section_presence: [false; N],
        }
    }
}
