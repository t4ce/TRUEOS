#![cfg_attr(not(test), no_std)]

#[cfg(any(feature = "alloc", test))]
extern crate alloc;

use core::fmt;

#[cfg(any(feature = "alloc", test))]
use alloc::vec::Vec;
use libm::sqrtf;

#[cfg(any(feature = "alloc", test))]
pub mod copy_kernel;

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Unsupported,
    Invalid,
    NotFound,
    OutOfMemory,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct BufferId(u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct ImageId(u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct SamplerId(u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct ShaderId(u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct PipelineId(u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct FenceId(u64);

impl BufferId {
    #[inline]
    pub const fn invalid() -> Self {
        Self(0)
    }

    #[inline]
    pub const fn is_valid(self) -> bool {
        self.0 != 0
    }

    #[inline]
    pub const fn raw(self) -> u32 {
        self.0
    }

    #[inline]
    pub const fn from_raw(raw: u32) -> Self {
        Self(raw)
    }
}

impl ImageId {
    #[inline]
    pub const fn invalid() -> Self {
        Self(0)
    }

    #[inline]
    pub const fn is_valid(self) -> bool {
        self.0 != 0
    }

    #[inline]
    pub const fn raw(self) -> u32 {
        self.0
    }

    #[inline]
    pub const fn from_raw(raw: u32) -> Self {
        Self(raw)
    }
}

impl SamplerId {
    #[inline]
    pub const fn invalid() -> Self {
        Self(0)
    }

    #[inline]
    pub const fn is_valid(self) -> bool {
        self.0 != 0
    }

    #[inline]
    pub const fn raw(self) -> u32 {
        self.0
    }

    #[inline]
    pub const fn from_raw(raw: u32) -> Self {
        Self(raw)
    }
}

impl ShaderId {
    #[inline]
    pub const fn invalid() -> Self {
        Self(0)
    }

    #[inline]
    pub const fn is_valid(self) -> bool {
        self.0 != 0
    }

    #[inline]
    pub const fn raw(self) -> u32 {
        self.0
    }

    #[inline]
    pub const fn from_raw(raw: u32) -> Self {
        Self(raw)
    }
}

impl PipelineId {
    #[inline]
    pub const fn invalid() -> Self {
        Self(0)
    }

    #[inline]
    pub const fn is_valid(self) -> bool {
        self.0 != 0
    }

    #[inline]
    pub const fn raw(self) -> u32 {
        self.0
    }

    #[inline]
    pub const fn from_raw(raw: u32) -> Self {
        Self(raw)
    }
}

impl FenceId {
    #[inline]
    pub const fn invalid() -> Self {
        Self(0)
    }

    #[inline]
    pub const fn is_valid(self) -> bool {
        self.0 != 0
    }

    #[inline]
    pub const fn raw(self) -> u64 {
        self.0
    }

    #[inline]
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShaderFormat {
    Tgsi,
    Nir,
    SpirV,
    Unknown(u32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShaderStage {
    Vertex,
    Fragment,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShaderDesc<'a> {
    pub stage: ShaderStage,
    pub format: ShaderFormat,
    pub bytes: &'a [u8],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BufferUsage {
    Vertex,
    Index,
    Uniform,
    TransferSrc,
    TransferDst,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryType {
    Device,
    HostVisible,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BufferDesc {
    pub size: u64,
    pub usage: BufferUsage,
    pub memory: MemoryType,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ColorFormat {
    RgbU8,
    RgbaU8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TexCoordFormat {
    None,
    UvF32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VertexLayout {
    pub stride: u16,
    pub pos_offset: u16,
    pub color_offset: u16,
    pub color_format: ColorFormat,
    pub texcoord_offset: u16,
    pub texcoord_format: TexCoordFormat,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PipelineDesc {
    pub vertex_layout: VertexLayout,
    pub vs: Option<ShaderId>,
    pub fs: Option<ShaderId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageFormat {
    Rgbx8888,
    Rgba8888,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UiSurfaceFormat {
    Rgba8888,
    Xrgb8888,
    Xbgr8888,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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
pub struct UiRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl UiRect {
    #[inline]
    pub const fn new(x: u32, y: u32, w: u32, h: u32) -> Self {
        Self { x, y, w, h }
    }

    #[inline]
    pub const fn is_empty(self) -> bool {
        self.w == 0 || self.h == 0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UiPresent {
    pub src: UiRect,
    pub dst: UiRect,
    pub plane: UiPlaneSlot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UiPresentPath {
    PlaneSourceOffset,
    KernelBlit,
    CpuCopy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImageDesc {
    pub width: u32,
    pub height: u32,
    pub format: ImageFormat,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImageRegion {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Extent2D {
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SwapchainDesc {
    pub format: ImageFormat,
    pub extent: Extent2D,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Viewport {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScissorRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MapMode {
    Read,
    Write,
    ReadWrite,
}

#[derive(Clone, Copy)]
pub struct MappedRange {
    pub ptr: *mut u8,
    pub len: usize,
}

impl fmt::Debug for MappedRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MappedRange")
            .field("ptr", &self.ptr)
            .field("len", &self.len)
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeviceCaps {
    pub supports_rgbx8888: bool,
    pub supports_host_visible_buffers: bool,
    pub supports_scissor: bool,
}

impl DeviceCaps {
    pub const fn minimal_software() -> Self {
        Self {
            supports_rgbx8888: true,
            supports_host_visible_buffers: true,
            supports_scissor: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Command {
    ClearColor {
        rgb: u32,
    },
    ClearColorRgba {
        rgba: Rgba8,
    },
    ClearRect {
        rgb: u32,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    },
    BindPipeline(PipelineId),
    BindVertexBuffer {
        buffer: BufferId,
        offset: u64,
    },
    BindImage(ImageId),
    SetRenderTarget(Option<ImageId>),
    SetSampler(SamplerDesc),
    SetBlend(BlendDesc),
    SetViewport(Viewport),
    SetScissor(Option<ScissorRect>),
    Draw {
        vertex_count: u32,
        first_vertex: u32,
    },
    Present,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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
pub struct BlendDesc {
    pub enabled: bool,
    pub src: BlendFactor,
    pub dst: BlendFactor,
}

impl BlendDesc {
    pub const fn disabled() -> Self {
        Self {
            enabled: false,
            src: BlendFactor::One,
            dst: BlendFactor::Zero,
        }
    }

    pub const fn straight_alpha() -> Self {
        Self {
            enabled: true,
            src: BlendFactor::SrcAlpha,
            dst: BlendFactor::OneMinusSrcAlpha,
        }
    }

    pub const fn premultiplied_alpha() -> Self {
        Self {
            enabled: true,
            src: BlendFactor::One,
            dst: BlendFactor::OneMinusSrcAlpha,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SamplerWrap {
    ClampToEdge,
    Repeat,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SamplerFilter {
    Nearest,
    Linear,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SamplerDesc {
    pub wrap_s: SamplerWrap,
    pub wrap_t: SamplerWrap,
    pub min_filter: SamplerFilter,
    pub mag_filter: SamplerFilter,
}

impl SamplerDesc {
    pub const fn default_2d() -> Self {
        Self {
            wrap_s: SamplerWrap::ClampToEdge,
            wrap_t: SamplerWrap::ClampToEdge,
            min_filter: SamplerFilter::Nearest,
            mag_filter: SamplerFilter::Nearest,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CommandBuffer<'a> {
    pub commands: &'a [Command],
}

#[cfg(any(feature = "alloc", test))]
pub struct CommandList {
    commands: alloc::vec::Vec<Command>,
}

#[cfg(any(feature = "alloc", test))]
impl CommandList {
    #[inline]
    pub fn new() -> Self {
        Self {
            commands: alloc::vec::Vec::new(),
        }
    }

    #[inline]
    pub fn push(&mut self, cmd: Command) {
        self.commands.push(cmd);
    }

    #[inline]
    pub fn as_buffer(&self) -> CommandBuffer<'_> {
        CommandBuffer {
            commands: &self.commands,
        }
    }
}

pub trait GfxDevice {
    fn caps(&self) -> DeviceCaps;

    fn create_buffer(&mut self, desc: BufferDesc) -> Result<BufferId>;
    fn destroy_buffer(&mut self, id: BufferId);

    fn create_shader(&mut self, desc: ShaderDesc<'_>) -> Result<ShaderId>;
    fn destroy_shader(&mut self, id: ShaderId);

    fn create_pipeline(&mut self, desc: PipelineDesc) -> Result<PipelineId>;
    fn destroy_pipeline(&mut self, id: PipelineId);

    fn create_image(&mut self, desc: ImageDesc) -> Result<ImageId>;
    fn destroy_image(&mut self, id: ImageId);
    fn write_image(&mut self, id: ImageId, data: &[u8]) -> Result<()>;

    fn write_image_region(
        &mut self,
        _id: ImageId,
        _region: ImageRegion,
        _data: &[u8],
    ) -> Result<()> {
        Err(Error::Unsupported)
    }

    fn write_buffer(&mut self, id: BufferId, offset: u64, data: &[u8]) -> Result<()>;

    fn map_buffer(&mut self, _id: BufferId, _mode: MapMode) -> Result<MappedRange> {
        Err(Error::Unsupported)
    }

    fn unmap_buffer(&mut self, _id: BufferId) -> Result<()> {
        Err(Error::Unsupported)
    }

    fn submit(&mut self, cmds: CommandBuffer<'_>) -> Result<FenceId>;
    fn poll(&mut self, fence: FenceId) -> bool;
    fn device_idle(&mut self);
}

pub trait GfxPresent {
    fn configure_swapchain(&mut self, desc: SwapchainDesc) -> Result<()>;
    fn swapchain_desc(&self) -> SwapchainDesc;

    /// Best-effort display refresh rate in millihertz.
    ///
    /// Backends that cannot query mode timing (or where "refresh" is not a meaningful
    /// hardware concept) should return `None`.
    #[inline]
    fn display_refresh_millihz(&mut self) -> Option<u32> {
        None
    }
}

/// Convenience trait for backends that implement both the device and present sides.
///
/// This keeps the decoupling seam (`GfxDevice` vs `GfxPresent`) while allowing
/// callers to borrow a single mutable context when the backend is a single object.
pub trait GfxContext: GfxDevice + GfxPresent {}

impl<T: GfxDevice + GfxPresent + ?Sized> GfxContext for T {}

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
    pub fn scale_alpha(self, alpha: u8) -> Self {
        let a = ((self.a as u16) * (alpha as u16) + 127) / 255;
        Self { a: a as u8, ..self }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub struct RgbVertex {
    pub x: f32,
    pub y: f32,
    pub color: Rgba8,
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub struct TexVertex {
    pub x: f32,
    pub y: f32,
    pub u: f32,
    pub v: f32,
    pub color: Rgba8,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RgbVertexF32 {
    pub x: f32,
    pub y: f32,
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TexVertexF32 {
    pub x: f32,
    pub y: f32,
    pub u: f32,
    pub v: f32,
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RgbVertexPx {
    pub x: f32,
    pub y: f32,
    pub color: Rgba8,
}

pub const RGB_VERTEX_SIZE: usize = core::mem::size_of::<RgbVertex>();
pub const TEX_VERTEX_SIZE: usize = core::mem::size_of::<TexVertex>();

#[inline]
fn clamp01(v: f32) -> f32 {
    if v <= 0.0 {
        0.0
    } else if v >= 1.0 {
        1.0
    } else {
        v
    }
}

#[inline]
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewTransform {
    pub width: f32,
    pub height: f32,
}

impl ViewTransform {
    #[inline]
    pub fn from_extent(width: u32, height: u32) -> Self {
        Self {
            width: width.max(1) as f32,
            height: height.max(1) as f32,
        }
    }

    #[inline]
    pub fn px_to_ndc(self, x: f32, y: f32) -> (f32, f32) {
        ((2.0 * (x / self.width)) - 1.0, 1.0 - (2.0 * (y / self.height)))
    }

    #[inline]
    pub fn rgb_vertex_px(self, x: f32, y: f32, color: Rgba8) -> RgbVertex {
        let (x, y) = self.px_to_ndc(x, y);
        RgbVertex { x, y, color }
    }

    #[inline]
    pub fn rgb_vertex_ndc(self, x: f32, y: f32, color: Rgba8) -> RgbVertex {
        let _ = self;
        RgbVertex { x, y, color }
    }

    #[inline]
    pub fn tex_vertex_px(self, x: f32, y: f32, u: f32, v: f32, color: Rgba8) -> TexVertex {
        let (x, y) = self.px_to_ndc(x, y);
        TexVertex { x, y, u, v, color }
    }

    #[inline]
    pub fn tex_vertex_ndc(self, x: f32, y: f32, u: f32, v: f32, color: Rgba8) -> TexVertex {
        let _ = self;
        TexVertex { x, y, u, v, color }
    }
}

#[inline]
pub fn rgb_vertices_byte_len(vertex_count: usize) -> usize {
    vertex_count.saturating_mul(RGB_VERTEX_SIZE)
}

#[inline]
pub fn tex_vertices_byte_len(vertex_count: usize) -> usize {
    vertex_count.saturating_mul(TEX_VERTEX_SIZE)
}

#[inline]
pub fn read_rgb_vertex_bytes(bytes: &[u8], off: usize) -> Option<RgbVertex> {
    if off + RGB_VERTEX_SIZE > bytes.len() {
        return None;
    }
    Some(RgbVertex {
        x: f32::from_le_bytes([bytes[off], bytes[off + 1], bytes[off + 2], bytes[off + 3]]),
        y: f32::from_le_bytes([
            bytes[off + 4],
            bytes[off + 5],
            bytes[off + 6],
            bytes[off + 7],
        ]),
        color: Rgba8::new(bytes[off + 8], bytes[off + 9], bytes[off + 10], bytes[off + 11]),
    })
}

#[inline]
pub fn read_rgb_vertex_f32_bytes(bytes: &[u8], off: usize) -> Option<RgbVertexF32> {
    let vertex = read_rgb_vertex_bytes(bytes, off)?;
    Some(RgbVertexF32 {
        x: vertex.x,
        y: vertex.y,
        r: (vertex.color.r as f32) / 255.0,
        g: (vertex.color.g as f32) / 255.0,
        b: (vertex.color.b as f32) / 255.0,
        a: (vertex.color.a as f32) / 255.0,
    })
}

#[inline]
pub fn read_tex_vertex_bytes(bytes: &[u8], off: usize) -> Option<TexVertex> {
    if off + TEX_VERTEX_SIZE > bytes.len() {
        return None;
    }
    Some(TexVertex {
        x: f32::from_le_bytes([bytes[off], bytes[off + 1], bytes[off + 2], bytes[off + 3]]),
        y: f32::from_le_bytes([
            bytes[off + 4],
            bytes[off + 5],
            bytes[off + 6],
            bytes[off + 7],
        ]),
        u: f32::from_le_bytes([
            bytes[off + 8],
            bytes[off + 9],
            bytes[off + 10],
            bytes[off + 11],
        ]),
        v: f32::from_le_bytes([
            bytes[off + 12],
            bytes[off + 13],
            bytes[off + 14],
            bytes[off + 15],
        ]),
        color: Rgba8::new(bytes[off + 16], bytes[off + 17], bytes[off + 18], bytes[off + 19]),
    })
}

#[inline]
pub fn read_tex_vertex_f32_bytes(bytes: &[u8], off: usize) -> Option<TexVertexF32> {
    let vertex = read_tex_vertex_bytes(bytes, off)?;
    Some(TexVertexF32 {
        x: vertex.x,
        y: vertex.y,
        u: vertex.u,
        v: vertex.v,
        r: (vertex.color.r as f32) / 255.0,
        g: (vertex.color.g as f32) / 255.0,
        b: (vertex.color.b as f32) / 255.0,
        a: (vertex.color.a as f32) / 255.0,
    })
}

#[inline]
pub fn interp_rgb_vertex_f32(a: RgbVertexF32, b: RgbVertexF32, t: f32) -> RgbVertexF32 {
    RgbVertexF32 {
        x: lerp(a.x, b.x, t),
        y: lerp(a.y, b.y, t),
        r: lerp(a.r, b.r, t),
        g: lerp(a.g, b.g, t),
        b: lerp(a.b, b.b, t),
        a: lerp(a.a, b.a, t),
    }
}

#[inline]
pub fn scissor_to_ndc(scissor: ScissorRect, vp_w: u32, vp_h: u32) -> Option<(f32, f32, f32, f32)> {
    if vp_w == 0 || vp_h == 0 {
        return None;
    }
    let x0 = scissor.x.min(vp_w) as f32;
    let y0 = scissor.y.min(vp_h) as f32;
    let x1 = scissor.x.saturating_add(scissor.width).min(vp_w) as f32;
    let y1 = scissor.y.saturating_add(scissor.height).min(vp_h) as f32;
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let w = vp_w as f32;
    let h = vp_h as f32;
    let left = (x0 / w) * 2.0 - 1.0;
    let right = (x1 / w) * 2.0 - 1.0;
    let top = 1.0 - (y0 / h) * 2.0;
    let bottom = 1.0 - (y1 / h) * 2.0;
    Some((left, right, bottom, top))
}

#[cfg(any(feature = "alloc", test))]
#[inline]
pub fn push_rgb_vertex_bytes(out: &mut Vec<u8>, vertex: RgbVertex) {
    out.extend_from_slice(&vertex.x.to_le_bytes());
    out.extend_from_slice(&vertex.y.to_le_bytes());
    out.push(vertex.color.r);
    out.push(vertex.color.g);
    out.push(vertex.color.b);
    out.push(vertex.color.a);
}

#[cfg(any(feature = "alloc", test))]
fn clip_rgb_poly_edge(input: &[RgbVertexF32], edge: u8, bound: f32, out: &mut Vec<RgbVertexF32>) {
    out.clear();
    if input.is_empty() {
        return;
    }

    let mut prev = input[input.len() - 1];
    let mut prev_in = match edge {
        0 => prev.x >= bound,
        1 => prev.x <= bound,
        2 => prev.y >= bound,
        _ => prev.y <= bound,
    };

    for &cur in input {
        let cur_in = match edge {
            0 => cur.x >= bound,
            1 => cur.x <= bound,
            2 => cur.y >= bound,
            _ => cur.y <= bound,
        };

        if cur_in != prev_in {
            let denom = match edge {
                0 | 1 => cur.x - prev.x,
                _ => cur.y - prev.y,
            };
            if denom.abs() > 1e-6 {
                let t = match edge {
                    0 | 1 => (bound - prev.x) / denom,
                    _ => (bound - prev.y) / denom,
                };
                out.push(interp_rgb_vertex_f32(prev, cur, t));
            }
        }

        if cur_in {
            out.push(cur);
        }

        prev = cur;
        prev_in = cur_in;
    }
}

#[cfg(any(feature = "alloc", test))]
#[inline]
pub fn push_rgb_vertex_f32_bytes(out: &mut Vec<u8>, vertex: RgbVertexF32) {
    push_rgb_vertex_bytes(
        out,
        RgbVertex {
            x: vertex.x,
            y: vertex.y,
            color: Rgba8::new(
                (clamp01(vertex.r) * 255.0 + 0.5) as u8,
                (clamp01(vertex.g) * 255.0 + 0.5) as u8,
                (clamp01(vertex.b) * 255.0 + 0.5) as u8,
                (clamp01(vertex.a) * 255.0 + 0.5) as u8,
            ),
        },
    );
}

#[cfg(any(feature = "alloc", test))]
pub fn clip_rgb_triangles_to_scissor_bytes(
    src: &[u8],
    scissor: ScissorRect,
    vp_w: u32,
    vp_h: u32,
) -> Vec<u8> {
    const TRI_SIZE: usize = RGB_VERTEX_SIZE * 3;

    let Some((left, right, bottom, top)) = scissor_to_ndc(scissor, vp_w, vp_h) else {
        return Vec::new();
    };

    let mut out = Vec::with_capacity(src.len());
    let usable = src.len() - (src.len() % TRI_SIZE);
    let mut poly_a: Vec<RgbVertexF32> = Vec::with_capacity(8);
    let mut poly_b: Vec<RgbVertexF32> = Vec::with_capacity(8);

    let mut off = 0usize;
    while off + TRI_SIZE <= usable {
        let Some(v0) = read_rgb_vertex_f32_bytes(src, off) else {
            break;
        };
        let Some(v1) = read_rgb_vertex_f32_bytes(src, off + RGB_VERTEX_SIZE) else {
            break;
        };
        let Some(v2) = read_rgb_vertex_f32_bytes(src, off + (2 * RGB_VERTEX_SIZE)) else {
            break;
        };
        off += TRI_SIZE;

        poly_a.clear();
        poly_a.push(v0);
        poly_a.push(v1);
        poly_a.push(v2);

        clip_rgb_poly_edge(&poly_a, 0, left, &mut poly_b);
        if poly_b.len() < 3 {
            continue;
        }
        clip_rgb_poly_edge(&poly_b, 1, right, &mut poly_a);
        if poly_a.len() < 3 {
            continue;
        }
        clip_rgb_poly_edge(&poly_a, 2, bottom, &mut poly_b);
        if poly_b.len() < 3 {
            continue;
        }
        clip_rgb_poly_edge(&poly_b, 3, top, &mut poly_a);
        if poly_a.len() < 3 {
            continue;
        }

        let base = poly_a[0];
        for i in 1..(poly_a.len() - 1) {
            push_rgb_vertex_f32_bytes(&mut out, base);
            push_rgb_vertex_f32_bytes(&mut out, poly_a[i]);
            push_rgb_vertex_f32_bytes(&mut out, poly_a[i + 1]);
        }
    }

    out
}

#[cfg(any(feature = "alloc", test))]
#[inline]
pub fn push_rgb_vertices_bytes(out: &mut Vec<u8>, vertices: &[RgbVertex]) {
    out.reserve(rgb_vertices_byte_len(vertices.len()));
    for &vertex in vertices {
        push_rgb_vertex_bytes(out, vertex);
    }
}

#[cfg(any(feature = "alloc", test))]
#[inline]
pub fn push_tex_vertex_bytes(out: &mut Vec<u8>, vertex: TexVertex) {
    out.extend_from_slice(&vertex.x.to_le_bytes());
    out.extend_from_slice(&vertex.y.to_le_bytes());
    out.extend_from_slice(&vertex.u.to_le_bytes());
    out.extend_from_slice(&vertex.v.to_le_bytes());
    out.push(vertex.color.r);
    out.push(vertex.color.g);
    out.push(vertex.color.b);
    out.push(vertex.color.a);
}

#[cfg(any(feature = "alloc", test))]
#[inline]
pub fn push_tex_vertices_bytes(out: &mut Vec<u8>, vertices: &[TexVertex]) {
    out.reserve(tex_vertices_byte_len(vertices.len()));
    for &vertex in vertices {
        push_tex_vertex_bytes(out, vertex);
    }
}

#[cfg(any(feature = "alloc", test))]
#[inline]
pub fn push_rgb_quad_px(
    out: &mut Vec<u8>,
    transform: ViewTransform,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
    color: Rgba8,
) {
    if !(left < right && top < bottom) {
        return;
    }
    push_rgb_vertex_bytes(out, transform.rgb_vertex_px(left, top, color));
    push_rgb_vertex_bytes(out, transform.rgb_vertex_px(right, top, color));
    push_rgb_vertex_bytes(out, transform.rgb_vertex_px(right, bottom, color));
    push_rgb_vertex_bytes(out, transform.rgb_vertex_px(left, top, color));
    push_rgb_vertex_bytes(out, transform.rgb_vertex_px(right, bottom, color));
    push_rgb_vertex_bytes(out, transform.rgb_vertex_px(left, bottom, color));
}

#[cfg(any(feature = "alloc", test))]
#[inline]
pub fn push_rgb_triangle_px(
    out: &mut Vec<u8>,
    transform: ViewTransform,
    v0: RgbVertexPx,
    v1: RgbVertexPx,
    v2: RgbVertexPx,
) {
    push_rgb_vertex_bytes(out, transform.rgb_vertex_px(v0.x, v0.y, v0.color));
    push_rgb_vertex_bytes(out, transform.rgb_vertex_px(v1.x, v1.y, v1.color));
    push_rgb_vertex_bytes(out, transform.rgb_vertex_px(v2.x, v2.y, v2.color));
}

#[cfg(any(feature = "alloc", test))]
pub fn push_indexed_rgb_mesh_px(
    out: &mut Vec<u8>,
    transform: ViewTransform,
    vertices: &[RgbVertexPx],
    indices: &[u16],
) {
    out.reserve(rgb_vertices_byte_len(indices.len()));
    for &idx in indices {
        let Some(vertex) = vertices.get(idx as usize) else {
            continue;
        };
        push_rgb_vertex_bytes(out, transform.rgb_vertex_px(vertex.x, vertex.y, vertex.color));
    }
}

#[cfg(any(feature = "alloc", test))]
#[inline]
pub fn push_rgb_quad_ndc(
    out: &mut Vec<u8>,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
    color: Rgba8,
) {
    if !(left < right && bottom < top) {
        return;
    }
    let verts = [
        RgbVertex {
            x: left,
            y: top,
            color,
        },
        RgbVertex {
            x: right,
            y: top,
            color,
        },
        RgbVertex {
            x: right,
            y: bottom,
            color,
        },
        RgbVertex {
            x: left,
            y: top,
            color,
        },
        RgbVertex {
            x: right,
            y: bottom,
            color,
        },
        RgbVertex {
            x: left,
            y: bottom,
            color,
        },
    ];
    push_rgb_vertices_bytes(out, &verts);
}

#[cfg(any(feature = "alloc", test))]
#[inline]
pub fn push_tex_quad_px(
    out: &mut Vec<u8>,
    transform: ViewTransform,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
    uv: [f32; 4],
    color: Rgba8,
) {
    if !(left < right && top < bottom) {
        return;
    }
    let [u0, v0, u1, v1] = uv;
    push_tex_vertex_bytes(out, transform.tex_vertex_px(left, bottom, u0, v1, color));
    push_tex_vertex_bytes(out, transform.tex_vertex_px(right, bottom, u1, v1, color));
    push_tex_vertex_bytes(out, transform.tex_vertex_px(right, top, u1, v0, color));
    push_tex_vertex_bytes(out, transform.tex_vertex_px(left, bottom, u0, v1, color));
    push_tex_vertex_bytes(out, transform.tex_vertex_px(right, top, u1, v0, color));
    push_tex_vertex_bytes(out, transform.tex_vertex_px(left, top, u0, v0, color));
}

#[cfg(any(feature = "alloc", test))]
#[inline]
pub fn push_tex_quad_ndc(
    out: &mut Vec<u8>,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
    uv: [f32; 4],
    color: Rgba8,
) {
    if !(left < right && bottom < top) {
        return;
    }
    let [u0, v0, u1, v1] = uv;
    let verts = [
        TexVertex {
            x: left,
            y: top,
            u: u0,
            v: v0,
            color,
        },
        TexVertex {
            x: right,
            y: top,
            u: u1,
            v: v0,
            color,
        },
        TexVertex {
            x: right,
            y: bottom,
            u: u1,
            v: v1,
            color,
        },
        TexVertex {
            x: left,
            y: top,
            u: u0,
            v: v0,
            color,
        },
        TexVertex {
            x: right,
            y: bottom,
            u: u1,
            v: v1,
            color,
        },
        TexVertex {
            x: left,
            y: bottom,
            u: u0,
            v: v1,
            color,
        },
    ];
    push_tex_vertices_bytes(out, &verts);
}

#[cfg(any(feature = "alloc", test))]
#[inline]
pub fn push_rgb_line_quad_px(
    out: &mut Vec<u8>,
    transform: ViewTransform,
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    thickness: f32,
    color: Rgba8,
) {
    let dx = x2 - x1;
    let dy = y2 - y1;
    let len_sq = (dx * dx) + (dy * dy);
    if !len_sq.is_finite() || len_sq <= f32::EPSILON {
        return;
    }

    let half = (thickness * 0.5).max(0.5);
    if !half.is_finite() {
        return;
    }

    let inv_len = sqrtf(len_sq).recip();
    let nx = -dy * inv_len;
    let ny = dx * inv_len;
    let ox = nx * half;
    let oy = ny * half;

    push_rgb_vertex_bytes(out, transform.rgb_vertex_px(x1 + ox, y1 + oy, color));
    push_rgb_vertex_bytes(out, transform.rgb_vertex_px(x2 + ox, y2 + oy, color));
    push_rgb_vertex_bytes(out, transform.rgb_vertex_px(x2 - ox, y2 - oy, color));
    push_rgb_vertex_bytes(out, transform.rgb_vertex_px(x1 + ox, y1 + oy, color));
    push_rgb_vertex_bytes(out, transform.rgb_vertex_px(x2 - ox, y2 - oy, color));
    push_rgb_vertex_bytes(out, transform.rgb_vertex_px(x1 - ox, y1 - oy, color));
}
