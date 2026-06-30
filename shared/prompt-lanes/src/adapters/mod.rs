//! Render targets for a [`ParsedPrompt`](crate::ParsedPrompt). Each output
//! format gets its own submodule here; the trait and the parser never need
//! to change when a new format is added.

mod markdown;

pub use markdown::MarkdownAdapter;

use crate::model::ParsedPrompt;

/// Renders a [`ParsedPrompt`] to a specific output format.
pub trait Adapter {
    fn render(&self, parsed: &ParsedPrompt) -> String;
}
