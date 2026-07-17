//! Kernel-owned UI4 frame contract.
//!
//! This module describes what a producer needs from the display path. It does
//! not expose a guest/userspace ABI and it does not own presentation yet.

pub(crate) mod blueprint_text;
mod dummy_ui4_consumer;
mod frame_pool;
mod input_broker;
mod screenshot;
mod video_frame;
mod window_broker;

pub(crate) use dummy_ui4_consumer::dummy_ui4_consumer_service_task;
pub(crate) use frame_pool::{
    FramePoolError, FrameReadLease, FrameRgbaView, FrameSnapshot, FrameWriteLease, PublishedFrame,
    acquire_frame_buffer, acquire_published_frame, cancel_frame_buffer, create_frame,
    destroy_frame, frame_snapshot, gpgpu_rgba_surface, import_native_nv12_frame,
    publish_frame_buffer, published_native_nv12_view, published_rgba_view, release_published_frame,
    writable_native_nv12_view, writable_rgba_view,
};
pub(crate) use input_broker::{
    Ui4ButtonPhase, Ui4InputEvent, Ui4PanPhase, Ui4VisualRect, software_cursor_visuals,
    take_owner_input_events, ui4_input_service_task,
};
pub(crate) use screenshot::ui4_screenshot_service_task;
pub(crate) use video_frame::{
    DecodedNv12Source, DecodedRgbaProducer, DecodedRgbaWriteTarget, DecodedVideoFrameSpec,
    acquire_decoded_rgba_stream_target, cancel_decoded_rgba_stream_target,
    present_decoded_nv12_stream_frame, publish_decoded_rgba_stream_target,
    stop_decoded_nv12_stream,
};

pub(crate) use window_broker::{
    DamageRect, WindowBrokerError, WindowCreate, WindowId, WindowOwner, WindowPlacement,
    WindowPlane, WindowSessionId, WindowSnapshot, WindowState, acknowledge_window_frame,
    begin_window_session, close_window, create_window, finish_window_session, publish_window_frame,
    replace_window_frame, set_window_placement, visible_windows_for_output,
};

pub(crate) const OUTPUT_COUNT: usize = 4;
pub(crate) const UNIVERSAL_PLANE_COUNT: usize = 4;
/// Common window extent for the temporary UI4 boot consumers. Keeping the
/// Mandelbrot and decoded-video probes on one extent makes their placement and
/// later interactive-resize work exercise the same broker contract.
pub(crate) const BOOT_DEMO_FRAME_WIDTH: u32 = 768;
pub(crate) const BOOT_DEMO_FRAME_HEIGHT: u32 = 512;
pub(crate) const PRIMARY_PLANE_SLOT: usize = 0;
pub(crate) const ALPHA_OVERLAY_PLANE_SLOT: usize = 1;
pub(crate) const RGB_OVERLAY_PLANE_SLOT_2: usize = 2;
pub(crate) const RGB_OVERLAY_PLANE_SLOT_3: usize = 3;
// Compatibility aliases for the parked legacy direct-NV12 experiment. Normal
// UI4 video is converted into RGBA and does not reserve these plane roles.
pub(crate) const NV12_UV_PLANE_SLOT: usize = RGB_OVERLAY_PLANE_SLOT_2;
pub(crate) const NV12_Y_PLANE_SLOT: usize = RGB_OVERLAY_PLANE_SLOT_3;

/// Stable logical display identity. Routing to pipe A-D is display-driver state.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
#[repr(transparent)]
pub(crate) struct OutputId(u8);

impl OutputId {
    pub(crate) const fn from_slot(slot: usize) -> Option<Self> {
        if slot < OUTPUT_COUNT {
            Some(Self(slot as u8))
        } else {
            None
        }
    }

    pub(crate) const fn slot(self) -> usize {
        self.0 as usize
    }

    pub(crate) const fn name(self) -> &'static str {
        match self.0 {
            0 => "D01",
            1 => "D02",
            2 => "D03",
            3 => "D04",
            _ => "D-invalid",
        }
    }
}

/// The producer which will write a frame. This does not imply update cadence.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum FrameContent {
    Video,
    Image,
    CpuBlit,
    CopyEngine,
    FontScene2d,
    RenderScene3d,
}

/// Lifetime/update discipline of the pixels handed to scanout.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum FrameCadence {
    /// Pixels never change while this frame remains published.
    Immutable,
    /// Updates occur on damage/dirty events and must not touch the live front.
    Dirty,
    /// A producer may submit another frame while one is queued and one is live.
    Streaming,
}

impl FrameCadence {
    pub(crate) const fn buffering(self) -> FrameBuffering {
        match self {
            Self::Immutable => FrameBuffering::Single,
            Self::Dirty => FrameBuffering::Double,
            Self::Streaming => FrameBuffering::Triple,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u8)]
pub(crate) enum FrameBuffering {
    Single = 1,
    Double = 2,
    Triple = 3,
}

impl FrameBuffering {
    pub(crate) const fn count(self) -> usize {
        self as usize
    }
}

/// Opaque reference to a frame-pool allocation. UI4 never treats it as an
/// address, texture ID, or display-plane register value.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
#[repr(transparent)]
pub(crate) struct FrameHandle(u64);

impl FrameHandle {
    pub(crate) const fn from_raw(raw: u64) -> Option<Self> {
        if raw == 0 { None } else { Some(Self(raw)) }
    }

    pub(crate) const fn raw(self) -> u64 {
        self.0
    }
}

/// Native formats currently programmed by the Intel display driver.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum ScanoutFormat {
    /// Opaque 8:8:8:8 primary-plane storage in RGB order.
    Xrgb8888,
    /// Opaque 8:8:8:8 primary-plane storage in BGR order.
    Xbgr8888,
    /// Per-pixel alpha overlay storage; RGB is already multiplied by alpha.
    Rgba8888Premultiplied,
    /// Linear two-plane NV12 used by the currently proven video staging ring.
    Nv12Linear,
    /// Current hardware-decoder Y-tiled NV12 surface.
    Nv12YTile,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum Nv12Layout {
    Linear,
    YTiled,
}

/// One trusted native NV12 buffer imported into a UI4 frame. The display
/// allocator retains the backing allocation; UI4 owns producer/read leases.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct NativeNv12Surface {
    pub(crate) phys: u64,
    pub(crate) gpu: u64,
    pub(crate) virt: usize,
    pub(crate) byte_len: usize,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pitch_bytes: u32,
    pub(crate) uv_offset: usize,
    pub(crate) layout: Nv12Layout,
    pub(crate) pipeline_slot: usize,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum AlphaContract {
    Opaque,
    PerPixelPremultiplied,
    /// NV12 carries no per-pixel alpha; the display plane supplies one value.
    PlaneConstant,
}

impl ScanoutFormat {
    pub(crate) const fn alpha(self) -> AlphaContract {
        match self {
            Self::Xrgb8888 | Self::Xbgr8888 => AlphaContract::Opaque,
            Self::Rgba8888Premultiplied => AlphaContract::PerPixelPremultiplied,
            Self::Nv12Linear | Self::Nv12YTile => AlphaContract::PlaneConstant,
        }
    }

    pub(crate) const fn plane(self) -> PlaneAssignment {
        match self {
            Self::Xrgb8888 | Self::Xbgr8888 => PlaneAssignment::Primary {
                slot: PRIMARY_PLANE_SLOT as u8,
            },
            Self::Rgba8888Premultiplied => PlaneAssignment::AlphaOverlay {
                slot: ALPHA_OVERLAY_PLANE_SLOT as u8,
            },
            Self::Nv12Linear | Self::Nv12YTile => PlaneAssignment::LinkedNv12 {
                uv_slot: NV12_UV_PLANE_SLOT as u8,
                y_slot: NV12_Y_PLANE_SLOT as u8,
            },
        }
    }
}

/// Exact current pipe-local plane assignment. Cursor is deliberately separate.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum PlaneAssignment {
    Primary { slot: u8 },
    AlphaOverlay { slot: u8 },
    LinkedNv12 { uv_slot: u8, y_slot: u8 },
}

/// One color in UI4's native RGBA surface convention.
///
/// The stored RGB channels are already multiplied by alpha. Consumers should
/// normally use [`Self::from_straight_rgba`] so the conversion happens once at
/// the frame-contract boundary rather than in every compositor pass.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(C)]
pub(crate) struct PremultipliedRgba8 {
    r: u8,
    g: u8,
    b: u8,
    a: u8,
}

impl PremultipliedRgba8 {
    pub(crate) const TRANSPARENT: Self = Self {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };

    /// Convert a conventional straight-alpha RGBA color to native UI4
    /// premultiplied RGBA bytes, with round-to-nearest integer division.
    pub(crate) const fn from_straight_rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self {
            r: premultiply_channel(r, a),
            g: premultiply_channel(g, a),
            b: premultiply_channel(b, a),
            a,
        }
    }

    pub(crate) const fn to_native_bytes(self) -> [u8; 4] {
        [self.r, self.g, self.b, self.a]
    }
}

const fn premultiply_channel(channel: u8, alpha: u8) -> u8 {
    ((channel as u16 * alpha as u16 + 127) / 255) as u8
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct FrameSpec {
    pub(crate) output: OutputId,
    pub(crate) content: FrameContent,
    pub(crate) cadence: FrameCadence,
    pub(crate) format: ScanoutFormat,
    pub(crate) width: u32,
    pub(crate) height: u32,
    /// Optional initial color for every cadence-selected backing buffer.
    /// This does not publish the frame; normal acquire/publish ownership still
    /// applies. Currently valid only for premultiplied RGBA frames.
    pub(crate) base_color: Option<PremultipliedRgba8>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct FramePlan {
    pub(crate) output: OutputId,
    pub(crate) content: FrameContent,
    pub(crate) format: ScanoutFormat,
    pub(crate) alpha: AlphaContract,
    pub(crate) plane: PlaneAssignment,
    pub(crate) buffering: FrameBuffering,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) base_color: Option<PremultipliedRgba8>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum FramePlanError {
    EmptyExtent,
    BaseColorRequiresPremultipliedRgba,
    VideoRequiresRgbaOrNv12,
    Nv12RequiresVideo,
}

impl FramePlan {
    /// Resolve one request without allocating memory or changing a display plane.
    pub(crate) const fn from_spec(spec: FrameSpec) -> Result<Self, FramePlanError> {
        if spec.width == 0 || spec.height == 0 {
            return Err(FramePlanError::EmptyExtent);
        }
        if spec.base_color.is_some() && !matches!(spec.format, ScanoutFormat::Rgba8888Premultiplied)
        {
            return Err(FramePlanError::BaseColorRequiresPremultipliedRgba);
        }
        match (spec.content, spec.format) {
            (
                FrameContent::Video,
                ScanoutFormat::Rgba8888Premultiplied
                | ScanoutFormat::Nv12Linear
                | ScanoutFormat::Nv12YTile,
            ) => {}
            (FrameContent::Video, _) => {
                return Err(FramePlanError::VideoRequiresRgbaOrNv12);
            }
            (_, ScanoutFormat::Nv12Linear | ScanoutFormat::Nv12YTile) => {
                return Err(FramePlanError::Nv12RequiresVideo);
            }
            _ => {}
        }
        Ok(Self {
            output: spec.output,
            content: spec.content,
            format: spec.format,
            alpha: spec.format.alpha(),
            plane: spec.format.plane(),
            buffering: spec.cadence.buffering(),
            width: spec.width,
            height: spec.height,
            base_color: spec.base_color,
        })
    }
}

const _: () = {
    assert!(FrameCadence::Immutable.buffering().count() == 1);
    assert!(FrameCadence::Dirty.buffering().count() == 2);
    assert!(FrameCadence::Streaming.buffering().count() == 3);
    assert!(matches!(
        ScanoutFormat::Rgba8888Premultiplied.plane(),
        PlaneAssignment::AlphaOverlay { slot: 1 }
    ));
    assert!(matches!(
        ScanoutFormat::Nv12YTile.plane(),
        PlaneAssignment::LinkedNv12 {
            uv_slot: 2,
            y_slot: 3
        }
    ));
    assert!(PRIMARY_PLANE_SLOT == 0);
    assert!(ALPHA_OVERLAY_PLANE_SLOT == 1);
    assert!(NV12_UV_PLANE_SLOT == 2);
    assert!(NV12_Y_PLANE_SLOT == 3);
    let transparent = PremultipliedRgba8::TRANSPARENT;
    assert!(transparent.r == 0 && transparent.g == 0 && transparent.b == 0 && transparent.a == 0);
    let half = PremultipliedRgba8::from_straight_rgba(255, 128, 1, 128);
    assert!(half.r == 128 && half.g == 64 && half.b == 1 && half.a == 128);
    let opaque = PremultipliedRgba8::from_straight_rgba(12, 34, 56, 255);
    assert!(opaque.r == 12 && opaque.g == 34 && opaque.b == 56 && opaque.a == 255);
};
