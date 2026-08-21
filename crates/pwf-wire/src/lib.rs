//! Versioned protobuf contract shared by the PWF gRPC client and server.

#[allow(
    clippy::default_trait_access,
    clippy::doc_markdown,
    clippy::match_single_binding,
    clippy::too_many_lines,
    clippy::trivially_copy_pass_by_ref
)]
pub mod v1 {
    tonic::include_proto!("pwf.v1");
}

/// Encoded descriptors used by standard gRPC reflection.
pub const FILE_DESCRIPTOR_SET: &[u8] = tonic::include_file_descriptor_set!("pwf_descriptor");
