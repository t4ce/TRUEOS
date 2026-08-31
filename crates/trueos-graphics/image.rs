//! Central image decode API for encoded raster/vector assets.

use alloc::vec::Vec;

pub(crate) use super::jpeg_codec;
pub(crate) use super::png_codec;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) enum EncodedImageKind {
    Png,
    Jpeg,
    Unknown,
}

impl EncodedImageKind {
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpeg",
            Self::Unknown => "unknown",
        }
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) const fn decode_error_code(self) -> i32 {
        match self {
            Self::Png | Self::Jpeg => -7,
            Self::Unknown => -8,
        }
    }
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) struct DecodedRgbaImage {
    pub(crate) kind: EncodedImageKind,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) rgba: Vec<u8>,
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) const fn encoded_image_kind(content_type: infer::ContentTypeId) -> EncodedImageKind {
    if content_type.raw() == infer::ContentTypeId::PNG.raw() {
        EncodedImageKind::Png
    } else if content_type.raw() == infer::ContentTypeId::JPEG.raw() {
        EncodedImageKind::Jpeg
    } else {
        EncodedImageKind::Unknown
    }
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) fn decode_encoded_image_rgba(
    content_type: infer::ContentTypeId,
    bytes: &[u8],
) -> Result<DecodedRgbaImage, i32> {
    decode_encoded_image_kind_rgba(encoded_image_kind(content_type), bytes)
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) fn decode_encoded_image_kind_rgba(
    kind: EncodedImageKind,
    bytes: &[u8],
) -> Result<DecodedRgbaImage, i32> {
    match kind {
        EncodedImageKind::Png => png_codec::decode_png_rgba(bytes)
            .map(|decoded| DecodedRgbaImage {
                kind,
                width: decoded.width,
                height: decoded.height,
                rgba: decoded.rgba,
            })
            .map_err(|err| err.code()),
        EncodedImageKind::Jpeg => jpeg_codec::decode_jpeg_rgba(bytes)
            .map(|decoded| DecodedRgbaImage {
                kind,
                width: decoded.width,
                height: decoded.height,
                rgba: decoded.rgba,
            })
            .map_err(|err| err.code()),
        EncodedImageKind::Unknown => Err(kind.decode_error_code()),
    }
}
