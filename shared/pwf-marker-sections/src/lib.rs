//! Parses runtime-configured one-line section syntax and renders it through an [`Adapter`].

mod adapters;
mod configuration;
mod marker_sections;
mod model;
mod text;

pub use adapters::{Adapter, MarkdownAdapter};
pub use configuration::{
    MARKER_SECTION_HEADER_CHARACTER_LIMIT, MarkerSectionConfiguration,
    MarkerSectionConfigurationError, MarkerSectionDefinition, MarkerSectionDefinitionError,
};
pub use marker_sections::{ITEM_MARKER, parse};
pub use model::ParsedMarkerSections;
