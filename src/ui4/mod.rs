//! Kernel-owned UI4 frame contract.
//!
//! This module describes what a producer needs from the display path. It does
//! not expose a guest/userspace ABI and it does not own presentation yet.

mod dummy_ui4_consumer;
mod frame_pool;
mod input_broker;
mod window_broker;

pub(crate) use dummy_ui4_consumer::dummy_ui4_consumer_service_task;
pub(crate) use frame_pool::{
    acquire_frame_buffer, acquire_published_frame, cancel_frame_buffer, create_frame,
    destroy_frame, frame_snapshot, gpgpu_rgba_surface, publish_frame_buffer, published_rgba_view,
    release_published_frame, writable_rgba_view, FramePoolError, FrameReadLease, FrameRgbaView,
    FrameSnapshot, FrameWriteLease, PublishedFrame,
};
pub(crate) use input_broker::{
    software_cursor_visuals, take_owner_input_events, ui4_input_service_task, Ui4ButtonPhase,
    Ui4InputEvent, Ui4PanPhase, Ui4VisualRect,
};

pub(crate) use window_broker::{
    acknowledge_window_frame, begin_window_session, close_window, create_window,
    finish_window_session, publish_window_frame, replace_window_frame, set_window_placement,
    visible_windows_for_output, DamageRect, WindowBrokerError, WindowCreate, WindowId, WindowOwner,
    WindowPlacement, WindowSessionId, WindowSnapshot, WindowState,
};

pub(crate) const OUTPUT_COUNT: usize = 4;
pub(crate) const PRIMARY_PLANE_SLOT: usize = 0;
pub(crate) const ALPHA_OVERLAY_PLANE_SLOT: usize = 1;
pub(crate) const NV12_UV_PLANE_SLOT: usize = 2;
pub(crate) const NV12_Y_PLANE_SLOT: usize = 3;

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
        if raw == 0 {
            None
        } else {
            Some(Self(raw))
        }
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
    /// Current hardware-decoder Y-tiled NV12 surface.
    Nv12YTile,
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
            Self::Nv12YTile => AlphaContract::PlaneConstant,
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
            Self::Nv12YTile => PlaneAssignment::LinkedNv12 {
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

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct FrameSpec {
    pub(crate) output: OutputId,
    pub(crate) content: FrameContent,
    pub(crate) cadence: FrameCadence,
    pub(crate) format: ScanoutFormat,
    pub(crate) width: u32,
    pub(crate) height: u32,
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
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum FramePlanError {
    EmptyExtent,
    VideoRequiresNv12,
    Nv12RequiresVideo,
}

impl FramePlan {
    /// Resolve one request without allocating memory or changing a display plane.
    pub(crate) const fn from_spec(spec: FrameSpec) -> Result<Self, FramePlanError> {
        if spec.width == 0 || spec.height == 0 {
            return Err(FramePlanError::EmptyExtent);
        }
        match (spec.content, spec.format) {
            (FrameContent::Video, ScanoutFormat::Nv12YTile) => {}
            (FrameContent::Video, _) => return Err(FramePlanError::VideoRequiresNv12),
            (_, ScanoutFormat::Nv12YTile) => return Err(FramePlanError::Nv12RequiresVideo),
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
};
