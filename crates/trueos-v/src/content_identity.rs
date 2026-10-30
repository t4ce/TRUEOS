//! Read-only access to TRUEOS's durable content-identity registry.
//!
//! Applications dispatch on [`ContentTypeId`]. Registry text is descriptive
//! UI metadata only; filenames, MIME strings, and extensions are never native
//! identity authority.

pub use infer::{
    ContentTypeId, ContentTypeInfo, MatcherType, content_type_info, content_types,
    is_registered_content_type,
};

/// Soft compatibility proposal for an untyped foreign file. No filesystem I/O.
/// The final known extension must agree exactly with the complete byte detector.
pub fn infer_named_bytes(path: &str, bytes: &[u8]) -> Option<ContentTypeId> {
    let name = path.rsplit('/').next()?;
    let (_, extension) = name.rsplit_once('.')?;
    let expected = infer::content_type_from_extension(extension)?;
    (expected != ContentTypeId::BLOB && infer::detect_content_type(bytes) == Some(expected))
        .then_some(expected)
}
