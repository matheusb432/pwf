//! Domain-neutral vault primitives shared by the pwf engines.
//!
//! This crate owns frontmatter parsing, atomic writes, date stamping, id allocation, and generic
//! index text transforms. It does not model tasks or notes.

pub mod date;
pub mod frontmatter;
pub mod fs_atomic;
pub mod id;
pub mod index;
