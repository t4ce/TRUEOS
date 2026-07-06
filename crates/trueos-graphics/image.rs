//! Central image decode API for encoded raster/vector assets.

use alloc::vec::Vec;

pub(crate) use super::decoder;
pub(crate) use super::jpeg_codec;
pub(crate) use super::jpeg_layout;
pub(crate) use super::png_codec;
pub(crate) use super::png_decode_pool;
pub(crate) use super::svg;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum EncodedImageKind {
    Png,
    Jpeg,
    Svg,
    Unknown,
}

impl EncodedImageKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpeg",
            Self::Svg => "svg",
            Self::Unknown => "unknown",
        }
    }

    pub(crate) const fn decode_error_code(self) -> i32 {
        match self {
            Self::Png | Self::Jpeg | Self::Svg => -7,
            Self::Unknown => -8,
        }
    }
}

pub(crate) struct DecodedRgbaImage {
    pub(crate) kind: EncodedImageKind,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) rgba: Vec<u8>,
}

pub(crate) fn infer_encoded_image_kind(
    declared_kind: &str,
    url: &str,
    bytes: &[u8],
) -> EncodedImageKind {
    let kind = declared_kind.to_ascii_lowercase();
    let url = url.to_ascii_lowercase();
    if kind.contains("svg") || url.ends_with(".svg") || bytes.starts_with(b"<svg") {
        return EncodedImageKind::Svg;
    }
    if kind.contains("png") || url.ends_with(".png") || bytes.starts_with(b"\x89PNG\r\n\x1A\n") {
        return EncodedImageKind::Png;
    }
    if kind.contains("jpg")
        || kind.contains("jpeg")
        || url.ends_with(".jpg")
        || url.ends_with(".jpeg")
        || bytes.starts_with(&[0xFF, 0xD8, 0xFF])
    {
        return EncodedImageKind::Jpeg;
    }
    EncodedImageKind::Unknown
}

pub(crate) fn decode_encoded_image_rgba(
    declared_kind: &str,
    url: &str,
    bytes: &[u8],
) -> Result<DecodedRgbaImage, i32> {
    decode_encoded_image_kind_rgba(infer_encoded_image_kind(declared_kind, url, bytes), bytes)
}

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
        EncodedImageKind::Svg => {
            svg::render_svg_bytes_rgba(bytes).map(|(info, rgba)| DecodedRgbaImage {
                kind,
                width: info.width,
                height: info.height,
                rgba,
            })
        }
        EncodedImageKind::Unknown => Err(kind.decode_error_code()),
    }
}
