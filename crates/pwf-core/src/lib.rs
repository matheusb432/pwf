//! pwf-core — domain-agnostic vault primitives shared by the pwf engines.
//! Knows nothing of tasks vs. notes: config resolution, frontmatter, atomic
//! writes, project paths, id allocation, and generic index text transforms.

pub mod config;
pub mod date;
pub mod frontmatter;
pub mod fs_atomic;
pub mod id;
pub mod index;
pub mod paths;
