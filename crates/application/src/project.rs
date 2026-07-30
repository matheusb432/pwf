//! Provides managed-project application operations.

use pwf_models::project::Project;

pub mod add_project;
mod dto;
pub mod get_project;
pub mod list_projects;
pub mod load_active_projects;
mod logic;
pub mod pause_project;
pub mod rename_project;
pub mod resolve_runtime_path;
pub mod resume_project;

pub use dto::{ProjectFields, ProjectStateChange};
pub use logic::{ProjectRegistry, ProjectResolutionError};
