//! Provides managed-project application operations.

pub mod add;
mod dto;
pub mod get;
pub mod list;
pub mod load_active;
pub mod pause;
pub mod resolve_runtime_path;
pub mod resume;
mod runtime_path;
mod task_location;

pub use dto::{Project, ProjectStateChange};
