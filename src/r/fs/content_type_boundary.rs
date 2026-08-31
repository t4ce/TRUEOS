//! Foreign file-ingress content identity checks.
//!
//! Filenames have no semantic authority inside TRUEOSFS.  HTTP and FTP still
//! need a narrow compatibility bridge for foreign clients, so they translate
//! the final extension to a stable identity and require the byte detector to
//! produce that exact identity before admitting the write.

use infer::ContentTypeId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum IngressTypeError {
    MissingExtension,
    UnsupportedExtension,
    Mismatch {
        expected: ContentTypeId,
        observed: Option<ContentTypeId>,
    },
}

pub(crate) fn final_extension(path: &str) -> Option<&str> {
    let name = path.rsplit('/').next()?;
    let (_, extension) = name.rsplit_once('.')?;
    if extension.is_empty() {
        None
    } else {
        Some(extension)
    }
}

pub(crate) fn verify_named_bytes(
    path: &str,
    bytes: &[u8],
) -> Result<ContentTypeId, IngressTypeError> {
    let extension = final_extension(path).ok_or(IngressTypeError::MissingExtension)?;
    let expected = infer::content_type_from_extension(extension)
        .ok_or(IngressTypeError::UnsupportedExtension)?;

    // Blob is a deliberate admission decision, never a filename inference.
    if expected == ContentTypeId::BLOB {
        return Err(IngressTypeError::UnsupportedExtension);
    }

    let observed = infer::detect_content_type(bytes);
    if observed == Some(expected) {
        Ok(expected)
    } else {
        Err(IngressTypeError::Mismatch { expected, observed })
    }
}

pub(crate) fn mime_or_octet_stream(content_type: ContentTypeId) -> &'static str {
    infer::content_type_info(content_type)
        .map(|info| info.mime_type)
        .unwrap_or("application/octet-stream")
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\nrest";

    #[test]
    fn final_extension_uses_only_the_final_path_component() {
        assert_eq!(final_extension("dir.with.dot/image.PNG"), Some("PNG"));
        assert_eq!(final_extension("no-extension"), None);
        assert_eq!(final_extension("trailing."), None);
    }

    #[test]
    fn named_ingress_requires_detector_equality() {
        assert_eq!(verify_named_bytes("image.png", PNG), Ok(ContentTypeId::PNG));
        assert!(matches!(
            verify_named_bytes("image.jpg", PNG),
            Err(IngressTypeError::Mismatch { .. })
        ));
    }

    #[test]
    fn blob_cannot_be_selected_by_generic_extension_ingress() {
        assert_eq!(
            verify_named_bytes("opaque.bin", b"anything"),
            Err(IngressTypeError::UnsupportedExtension)
        );
    }
}
