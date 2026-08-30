use std::array;

/// Contains a parsed title and one ordered item collection per configured lane.
#[derive(Debug, PartialEq, Eq)]
pub struct ParsedPrompt<const N: usize> {
    pub(crate) title: String,
    pub(crate) lane_items: [Vec<String>; N],
}

impl<const N: usize> ParsedPrompt<N> {
    #[must_use]
    pub fn new(title: String, lane_items: [Vec<String>; N]) -> Self {
        Self { title, lane_items }
    }

    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    #[must_use]
    pub fn lane_items(&self) -> &[Vec<String>; N] {
        &self.lane_items
    }

    #[must_use]
    pub fn into_parts(self) -> (String, [Vec<String>; N]) {
        (self.title, self.lane_items)
    }
}

impl<const N: usize> Default for ParsedPrompt<N> {
    fn default() -> Self {
        Self {
            title: String::new(),
            lane_items: array::from_fn(|_| Vec::new()),
        }
    }
}
