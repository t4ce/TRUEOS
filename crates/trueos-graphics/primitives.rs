//! Shared graphics vocabulary and ABI-facing draw command types.
//!
//! This module is intentionally backend-neutral. It describes pixels, surface
//! layout, presentation rectangles, blend/sampler state, and compact UI draw
//! commands without owning GPU submission or device memory.

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    Unsupported,
    Invalid,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    NotFound,
    OutOfMemory,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UiSurfaceFormat {
    Rgba8888,
    Xrgb8888,
    Xbgr8888,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub enum UiPlaneSlot {
    Primary,
    Overlay(u8),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UiSurface {
    pub gpu: u64,
    pub width: u32,
    pub height: u32,
    pub pitch: u32,
    pub format: UiSurfaceFormat,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub struct UiRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl UiRect {
    #[inline]
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub const fn new(x: u32, y: u32, w: u32, h: u32) -> Self {
        Self { x, y, w, h }
    }

    #[inline]
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub const fn is_empty(self) -> bool {
        self.w == 0 || self.h == 0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub struct UiPresent {
    pub src: UiRect,
    pub dst: UiRect,
    pub plane: UiPlaneSlot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub enum UiPresentPath {
    PlaneSourceOffset,
    CpuCopy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub struct ScissorRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub enum BlendFactor {
    Zero,
    One,
    SrcAlpha,
    OneMinusSrcAlpha,
    DstColor,
    OneMinusDstColor,
    OneMinusSrcColor,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub struct BlendDesc {
    pub enabled: bool,
    pub src: BlendFactor,
    pub dst: BlendFactor,
}

impl BlendDesc {
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub const fn disabled() -> Self {
        Self {
            enabled: false,
            src: BlendFactor::One,
            dst: BlendFactor::Zero,
        }
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub const fn straight_alpha() -> Self {
        Self {
            enabled: true,
            src: BlendFactor::SrcAlpha,
            dst: BlendFactor::OneMinusSrcAlpha,
        }
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub const fn premultiplied_alpha() -> Self {
        Self {
            enabled: true,
            src: BlendFactor::One,
            dst: BlendFactor::OneMinusSrcAlpha,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub enum SamplerWrap {
    ClampToEdge,
    Repeat,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub enum SamplerFilter {
    Nearest,
    Linear,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub struct SamplerDesc {
    pub wrap_s: SamplerWrap,
    pub wrap_t: SamplerWrap,
    pub min_filter: SamplerFilter,
    pub mag_filter: SamplerFilter,
}

impl SamplerDesc {
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub const fn default_2d() -> Self {
        Self {
            wrap_s: SamplerWrap::ClampToEdge,
            wrap_t: SamplerWrap::ClampToEdge,
            min_filter: SamplerFilter::Nearest,
            mag_filter: SamplerFilter::Nearest,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct Rgba8 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba8 {
    #[inline]
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    #[inline]
    pub const fn from_rgba_u32(rgba: u32) -> Self {
        Self {
            r: ((rgba >> 24) & 0xFF) as u8,
            g: ((rgba >> 16) & 0xFF) as u8,
            b: ((rgba >> 8) & 0xFF) as u8,
            a: (rgba & 0xFF) as u8,
        }
    }

    #[inline]
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub fn scale_alpha(self, alpha: u8) -> Self {
        let a = ((self.a as u16) * (alpha as u16) + 127) / 255;
        Self { a: a as u8, ..self }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub struct SolidRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub color: Rgba8,
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub struct SpriteCorner {
    pub x: f32,
    pub y: f32,
    pub u: f32,
    pub v: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub struct SpriteQuad {
    pub c0: SpriteCorner,
    pub c1: SpriteCorner,
    pub c2: SpriteCorner,
    pub c3: SpriteCorner,
    pub color: Rgba8,
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub const SOLID_RECT_SIZE: usize = core::mem::size_of::<SolidRect>();
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub const SPRITE_QUAD_SIZE: usize = core::mem::size_of::<SpriteQuad>();

#[inline]
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
fn read_f32_le(bytes: &[u8], off: usize) -> Option<f32> {
    Some(f32::from_le_bytes([
        *bytes.get(off)?,
        *bytes.get(off + 1)?,
        *bytes.get(off + 2)?,
        *bytes.get(off + 3)?,
    ]))
}

#[inline]
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub fn read_solid_rect_bytes(bytes: &[u8], off: usize) -> Option<SolidRect> {
    if off + SOLID_RECT_SIZE > bytes.len() {
        return None;
    }
    Some(SolidRect {
        x: read_f32_le(bytes, off)?,
        y: read_f32_le(bytes, off + 4)?,
        w: read_f32_le(bytes, off + 8)?,
        h: read_f32_le(bytes, off + 12)?,
        color: Rgba8::new(bytes[off + 16], bytes[off + 17], bytes[off + 18], bytes[off + 19]),
    })
}

#[inline]
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub fn read_sprite_corner_bytes(bytes: &[u8], off: usize) -> Option<SpriteCorner> {
    Some(SpriteCorner {
        x: read_f32_le(bytes, off)?,
        y: read_f32_le(bytes, off + 4)?,
        u: read_f32_le(bytes, off + 8)?,
        v: read_f32_le(bytes, off + 12)?,
    })
}

#[inline]
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub fn read_sprite_quad_bytes(bytes: &[u8], off: usize) -> Option<SpriteQuad> {
    if off + SPRITE_QUAD_SIZE > bytes.len() {
        return None;
    }
    Some(SpriteQuad {
        c0: read_sprite_corner_bytes(bytes, off)?,
        c1: read_sprite_corner_bytes(bytes, off + 16)?,
        c2: read_sprite_corner_bytes(bytes, off + 32)?,
        c3: read_sprite_corner_bytes(bytes, off + 48)?,
        color: Rgba8::new(bytes[off + 64], bytes[off + 65], bytes[off + 66], bytes[off + 67]),
    })
}
