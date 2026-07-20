//! Kernel-owned UI4 frame contract.
//!
//! This module describes what a producer needs from the display path. It does
//! not expose a guest/userspace ABI; the compositor service owns presentation.

pub(crate) mod blueprint_text;
mod compositor_service;
mod damage;
mod frame_pool;
mod gpgpu_preview_consumer;
#[cfg(feature = "trueos_h264_encode_probe")]
mod h264_encode_probe;
mod input_broker;
mod screenshot;
mod slot4_service;
mod video_frame;
mod window_broker;

pub(crate) use compositor_service::ui4_compositor_service_task;
pub(crate) use damage::{DamageRect, DamageRegion};
pub(crate) use frame_pool::{
    FrameGpuRelease, FramePoolError, FrameReadLease, FrameRgbaView, FrameWriteLease,
    NativeVideoFrameView, acquire_frame_buffer, acquire_published_frame, cancel_frame_buffer,
    create_frame, destroy_frame, frame_snapshot, gpgpu_rgba_surface,
    mark_frame_buffer_cpu_authored, publish_frame_buffer, publish_gpgpu_frame_buffer,
    publish_gpgpu_native_video_frame_buffer, publish_gpgpu_scene_frame_buffer,
    publish_gpu_frame_buffer, publish_native_video_frame_buffer, published_native_video_view,
    published_rgba_view, release_published_frame, retain_published_frame,
    wait_frame_buffer_release, writable_rgba_view,
};
pub(crate) use gpgpu_preview_consumer::{
    GPGPU_PREVIEW_DEFAULT_CADENCE_MS, GPGPU_PREVIEW_DEFAULT_DURATION_MS,
    GPGPU_PREVIEW_DEFAULT_PUBLISH_EVERY, GpgpuPreviewConfig, GpgpuPreviewPreset,
    gpgpu_preview_consumer_service_task, gpgpu_preview_status, request_gpgpu_preview_start,
    request_gpgpu_preview_stop,
};
#[cfg(feature = "trueos_h264_encode_probe")]
pub(crate) use h264_encode_probe::ui4_h264_encode_probe_task;
pub(crate) use input_broker::{
    Ui4ButtonPhase, Ui4CursorSource, Ui4InputEvent, Ui4PanEvent, Ui4PanPhase, Ui4VisualRect,
    software_cursor_visuals, take_owner_input_events, ui4_input_service_task,
};
pub(crate) use screenshot::ui4_screenshot_service_task;
pub(crate) use slot4_service::ui4_slot4_service_task;
pub(crate) use video_frame::{
    DecodedNv12Source, acknowledge_native_video_publication, begin_shell_decoded_video_player,
    prepare_decoded_video_player, present_decoded_nv12_stream_frame, stop_decoded_nv12_stream,
    wait_decoded_video_playback_ready,
};

pub(crate) use window_broker::{
    WindowBrokerError, WindowCreate, WindowId, WindowInteraction, WindowOwner, WindowPlacement,
    WindowPlane, WindowSessionCloseRequest, WindowSessionId, WindowSnapshot, WindowState,
    acknowledge_window_frame, advance_window_close_transitions, begin_window_session, close_window,
    create_window, finish_window_session, finish_window_session_with_request, move_window,
    publish_window_frame, publish_window_frames, replace_window_frame, set_window_placement,
    toggle_window_maximized, visible_windows_for_output,
};

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct OwnerReleaseSummary {
    pub(crate) surfaces: usize,
    pub(crate) input_routes: usize,
    pub(crate) input_events: usize,
}

/// Release all UI4 resources belonging to an application owner.
///
/// UI4 deliberately does not infer owner liveness. The application lifecycle
/// calls this operation when an owner ceases to exist.
pub(crate) fn release_owner_resources(owner: WindowOwner) -> OwnerReleaseSummary {
    let surfaces = blueprint_text::release_owner_resources(owner);
    let (input_routes, input_events) = input_broker::release_owner(owner);
    OwnerReleaseSummary {
        surfaces,
        input_routes,
        input_events,
    }
}

pub(crate) const OUTPUT_COUNT: usize = 4;
pub(crate) const UNIVERSAL_PLANE_COUNT: usize = 5;
/// Default broker extent for kernel UI4 producers that do not yet negotiate a
/// size with an external application.
pub(crate) const DEFAULT_FRAME_WIDTH: u32 = 768;
pub(crate) const DEFAULT_FRAME_HEIGHT: u32 = 512;
pub(crate) const PRIMARY_PLANE_SLOT: usize = 0;
pub(crate) const ALPHA_OVERLAY_PLANE_SLOT: usize = 1;
pub(crate) const RGB_OVERLAY_PLANE_SLOT_2: usize = 2;
pub(crate) const RGB_OVERLAY_PLANE_SLOT_3: usize = 3;
/// Highest universal plane. UI4 reserves it for input chrome rather than
/// broker windows so cursors, selection outlines and context menus never
/// become part of an application composition surface.
pub(crate) const INTERACTION_OVERLAY_PLANE_SLOT: usize = 4;
// Compatibility aliases for the parked linked-NV12 display-plane experiment.
// Normal UI4 video is converted by the GuC into the primary XRGB swap surface;
// it never assigns decoder planes to either of these roles.
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
    BlueprintScene,
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

/// Pixel formats owned by ordinary UI4 Frame allocations. Native decoder
/// formats are render-source attachments, never implicit display-plane modes.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum ScanoutFormat {
    /// Opaque 8:8:8:8 primary-plane storage in RGB order.
    Xrgb8888,
    /// Opaque 8:8:8:8 primary-plane storage in BGR order.
    Xbgr8888,
    /// Per-pixel alpha overlay storage; RGB is already multiplied by alpha.
    Rgba8888Premultiplied,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum Nv12Layout {
    Linear,
    YTiled,
}

/// One trusted native NV12 render source. This type carries no plane
/// assignment: the GuC conversion consumes it while rebuilding the primary
/// XRGB swap surface.
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
}

impl ScanoutFormat {
    pub(crate) const fn alpha(self) -> AlphaContract {
        match self {
            Self::Xrgb8888 | Self::Xbgr8888 => AlphaContract::Opaque,
            Self::Rgba8888Premultiplied => AlphaContract::PerPixelPremultiplied,
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
        }
    }
}

/// Exact current pipe-local plane assignment. Cursor is deliberately separate.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum PlaneAssignment {
    Primary { slot: u8 },
    AlphaOverlay { slot: u8 },
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
    VideoRequiresPremultipliedRgba,
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
            (FrameContent::Video, ScanoutFormat::Rgba8888Premultiplied) => {}
            (FrameContent::Video, _) => {
                return Err(FramePlanError::VideoRequiresPremultipliedRgba);
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
    assert!(PRIMARY_PLANE_SLOT == 0);
    assert!(ALPHA_OVERLAY_PLANE_SLOT == 1);
    assert!(NV12_UV_PLANE_SLOT == 2);
    assert!(NV12_Y_PLANE_SLOT == 3);
    assert!(INTERACTION_OVERLAY_PLANE_SLOT == 4);
    let transparent = PremultipliedRgba8::TRANSPARENT;
    assert!(transparent.r == 0 && transparent.g == 0 && transparent.b == 0 && transparent.a == 0);
    let half = PremultipliedRgba8::from_straight_rgba(255, 128, 1, 128);
    assert!(half.r == 128 && half.g == 64 && half.b == 1 && half.a == 128);
    let opaque = PremultipliedRgba8::from_straight_rgba(12, 34, 56, 255);
    assert!(opaque.r == 12 && opaque.g == 34 && opaque.b == 56 && opaque.a == 255);
};
