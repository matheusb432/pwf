//! Provides managed-project application operations.

pub mod add;
mod dto;
pub mod get;
pub mod list;
pub mod pause;
pub mod resume;

pub use dto::{Project, ProjectStateChange};
