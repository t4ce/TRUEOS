//! Read-only access to TRUEOS's durable content-identity registry.
//!
//! Applications dispatch on [`ContentTypeId`]. Registry text is descriptive
//! UI metadata only; filenames, MIME strings, and extensions are never native
//! identity authority.

pub use infer::{
    ContentTypeId, ContentTypeInfo, MatcherType, content_type_info, content_types,
    is_registered_content_type,
};
