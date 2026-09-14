//! Render targets for a [`ParsedMarkerSections`](crate::ParsedMarkerSections).

mod markdown;

pub use markdown::MarkdownAdapter;

use crate::model::ParsedMarkerSections;

/// Renders a [`ParsedMarkerSections`] to a specific output format.
pub trait Adapter<const N: usize> {
    fn render(&self, parsed: &ParsedMarkerSections<N>) -> String;
}
