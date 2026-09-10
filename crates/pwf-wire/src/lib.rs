//! Process-neutral operation contracts and their versioned protobuf representation.

pub mod collection_edit;
pub mod confirmation;
pub mod doctor;
pub mod note;
pub mod pagination;
pub mod patch_field;
pub mod project;
pub mod proto;
pub mod set_field;
pub mod task;

#[allow(
    clippy::default_trait_access,
    clippy::doc_markdown,
    clippy::excessive_nesting,
    clippy::match_single_binding,
    clippy::must_use_candidate,
    clippy::too_many_lines,
    clippy::trivially_copy_pass_by_ref
)]
pub mod pb {
    include!("generated/pwf.v1.rs");
}

/// Encoded descriptors used by standard gRPC reflection.
pub const FILE_DESCRIPTOR_SET: &[u8] = include_bytes!("generated/pwf_descriptor.bin");
