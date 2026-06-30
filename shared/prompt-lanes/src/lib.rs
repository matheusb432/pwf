//! prompt-lanes: a domain-agnostic parser for the one-line lane-syntax
//! task-prompt format (`title / goal /c context /n constraint /d done when`),
//! rendered through a pluggable [`Adapter`](adapters::Adapter).

mod adapters;
mod lanes;
mod model;
mod title;

pub use adapters::{Adapter, MarkdownAdapter};
pub use lanes::parse;
pub use model::ParsedPrompt;
