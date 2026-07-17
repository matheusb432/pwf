//! Parses one-line lane syntax and renders it through an [`Adapter`].
//!
//! The syntax is `title / goal /c context /n constraint /d done when`.

mod adapters;
mod lanes;
mod model;
mod title;

pub use adapters::{Adapter, MarkdownAdapter};
pub use lanes::parse;
pub use model::ParsedPrompt;
