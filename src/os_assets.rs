//! Read-only registry of the kernel’s directly embedded display images.
//! Firmware BGRT and decoded live-update panel views remain opt-in sources.

pub(crate) const LOGO_JPEG: &[u8] = include_bytes!("../logo.jpg");
pub(crate) const INTEL_GRAPHICS_LOGO_PNG: &[u8] = include_bytes!("../Intel_Graphics_logo.png");
pub(crate) const LIVE_UPDATE_0WAY_PNG: &[u8] = include_bytes!("../tools/updlive/0way.png");
pub(crate) const LIVE_UPDATE_NOWAY_PNG: &[u8] = include_bytes!("../tools/updlive/noway.png");
pub(crate) const LIVE_UPDATE_NWAY_PNG: &[u8] = include_bytes!("../tools/updlive/nway.png");

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
        "kernel:live-update-0way" => (3, 1122, 1402, LIVE_UPDATE_0WAY_PNG),
        "kernel:live-update" | "kernel:live-update-noway" => (3, 1122, 1402, LIVE_UPDATE_NOWAY_PNG),
        "kernel:live-update-nway" => (3, 1536, 1024, LIVE_UPDATE_NWAY_PNG),
        _ => return None,
    };
    Some(EmbeddedImage {
        format,
        width,
        height,
        bytes,
    })
}
