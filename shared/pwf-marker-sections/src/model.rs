/// Contains a parsed title, marker presence, and ordered items for each configured section.
#[derive(Debug, PartialEq, Eq)]
pub struct ParsedMarkerSections {
    pub(crate) title: String,
    pub(crate) section_items: Vec<Vec<String>>,
    pub(crate) section_presence: Vec<bool>,
}

impl ParsedMarkerSections {
    pub(crate) fn empty(section_count: usize) -> Self {
        Self {
            title: String::new(),
            section_items: vec![Vec::new(); section_count],
            section_presence: vec![false; section_count],
        }
    }

    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Returns items in configured section order.
    #[must_use]
    pub fn section_items(&self) -> &[Vec<String>] {
        &self.section_items
    }

    /// Treats only sections with items as present, dropping explicitly marked empty sections.
    #[must_use]
    pub fn omit_empty_sections(mut self) -> Self {
        for (presence, items) in self.section_presence.iter_mut().zip(&self.section_items) {
            *presence = !items.is_empty();
        }
        self
    }

    /// Returns the title and items without the original marker-presence metadata.
    #[must_use]
    pub fn into_parts(self) -> (String, Vec<Vec<String>>) {
        (self.title, self.section_items)
    }
}
