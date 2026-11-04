//! Read-only registry of the kernel’s directly embedded display images.
//! Firmware BGRT remains an opt-in source.

pub(crate) const LOGO_JPEG: &[u8] = include_bytes!("../logo.jpg");
pub(crate) const INTEL_GRAPHICS_LOGO_PNG: &[u8] = include_bytes!("../Intel_Graphics_logo.png");

pub(crate) struct EmbeddedImage {
    pub format: u32,
    pub width: u32,
    pub height: u32,
    pub bytes: &'static [u8],
}

pub(crate) fn image(name: &str) -> Option<EmbeddedImage> {
    let (format, width, height, bytes) = match name {
        "kernel:logo" => (1, 3840, 2160, LOGO_JPEG),
        "kernel:intel-graphics" => (3, 565, 565, INTEL_GRAPHICS_LOGO_PNG),
        _ => return None,
    };
    Some(EmbeddedImage {
        format,
        width,
        height,
        bytes,
    })
}
