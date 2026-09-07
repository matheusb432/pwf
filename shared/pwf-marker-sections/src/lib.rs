//! Parses runtime-configured one-line lane syntax and renders it through an [`Adapter`].

mod adapters;
mod configuration;
mod lanes;
mod model;
mod text;

pub use adapters::{Adapter, MarkdownAdapter};
pub use configuration::{
    LANE_HEADER_CHARACTER_LIMIT, LaneConfiguration, LaneConfigurationError, LaneDefinition,
    LaneDefinitionError,
};
pub use lanes::{ITEM_MARKER, parse};
pub use model::ParsedPrompt;
