//! Render targets for a [`ParsedPrompt`](crate::ParsedPrompt).

mod markdown;

pub use markdown::MarkdownAdapter;

use crate::model::ParsedPrompt;

/// Renders a [`ParsedPrompt`] to a specific output format.
pub trait Adapter<const N: usize> {
    fn render(&self, parsed: &ParsedPrompt<N>) -> String;
}
