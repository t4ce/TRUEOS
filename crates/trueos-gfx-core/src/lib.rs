#![cfg_attr(not(test), no_std)]

#[cfg(any(feature = "alloc", test))]
extern crate alloc;

use core::fmt;

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
pub struct ImageDesc {
    pub width: u32,
    pub height: u32,
    pub format: ImageFormat,
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
