//! pw-workflow engines: pending-work, handoff, and migrate. Behavior is pinned by
//! the language-neutral golden harness in `conformance/`.
#![warn(clippy::all)]

pub mod cli;
pub mod command;
pub mod config;
pub mod engines;
pub mod frontmatter;
pub mod fs_atomic;
pub mod help;
pub mod preprocess;
