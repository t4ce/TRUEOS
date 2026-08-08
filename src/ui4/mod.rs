//! Kernel-owned UI4 frame contract.
//!
//! This module describes what a producer needs from the display path. It does
//! not expose a guest/userspace ABI; the compositor service owns presentation.

pub(crate) mod blueprint_text;
mod color_picker;
mod compositor_service;
mod context_menu;
mod cursor_frame_inout;
mod damage;
mod frame_pool;
mod gpgpu_preview_consumer;
mod gpgpu_svg_probe_consumer;
#[cfg(feature = "trueos_h264_encode_stream")]
mod h264_encode_stream;
#[cfg(feature = "trueos_h264_encode_stream")]
mod h264_encode_udp;
mod input_broker;
mod screenshot;
mod slot4_service;
mod video_frame;
mod window_broker;
pub(crate) mod winit_input;

const INTERACTION_CADENCE_HZ: u64 = 60;

/// Absolute 60 Hz deadlines represented on Embassy's 1 kHz clock. The fractional
/// remainder produces a 16/17/17 ms pattern instead of rounding every period to
/// 17 ms (58.8 Hz), and rebases after a delayed executor turn rather than bursting.
struct InteractionCadence {
    next_tick: u64,
    remainder: u64,
}

impl InteractionCadence {
    fn new() -> Self {
        Self {
            next_tick: embassy_time::Instant::now().as_ticks(),
            remainder: 0,
        }
    }

    fn next_deadline(&mut self) -> embassy_time::Instant {
        let now_tick = embassy_time::Instant::now().as_ticks();
        if self.next_tick < now_tick {
            self.next_tick = now_tick;
            self.remainder = 0;
        }

        let tick_hz = embassy_time::TICK_HZ;
        let mut period_ticks = tick_hz / INTERACTION_CADENCE_HZ;
        self.remainder = self
            .remainder
            .saturating_add(tick_hz % INTERACTION_CADENCE_HZ);
        period_ticks = period_ticks.saturating_add(self.remainder / INTERACTION_CADENCE_HZ);
        self.remainder %= INTERACTION_CADENCE_HZ;
        self.next_tick = self.next_tick.saturating_add(period_ticks.max(1));
        embassy_time::Instant::from_ticks(self.next_tick)
    }
}

pub(crate) use color_picker::ui4_color_picker_service_task;
pub(crate) use compositor_service::ui4_compositor_service_task;
pub(crate) use context_menu::{
    ContextMenuCloseReason, ContextMenuEntry, ContextMenuError, ContextMenuRequest,
    ContextMenuResult, MAX_CONTEXT_MENU_ENTRIES, clear_window_menu as clear_window_context_menu,
    register_window_menu as register_window_context_menu,
};
pub(crate) use cursor_frame_inout::{
    CursorFrameKey, GlobalKeyboardDisposition, GlobalKeyboardHookId, Ui4CursorIcon,
    Ui4CursorSource, cursor_color, register_global_keyboard_hook, selected_frame,
    selected_frame_for_source, selection_strips, set_window_cursor_icon, set_window_custom_cursor,
    unregister_global_keyboard_hook,
};
pub(crate) use damage::{DamageRect, DamageRegion};
pub(crate) use frame_pool::{
    FrameGpuRelease, FramePoolError, FrameReadLease, FrameRgbaView, FrameWriteLease,
    acquire_frame_buffer, acquire_published_frame, cancel_frame_buffer, create_frame,
    create_gpu_full_overwrite_frame, destroy_frame, frame_buffer_ownership_probe, frame_snapshot,
    gpgpu_rgba_surface, mark_frame_buffer_cpu_authored, mark_frame_buffer_fully_opaque,
    publish_frame_buffer,
    publish_gpgpu_frame_buffer, publish_gpgpu_render_frame_buffer,
    publish_gpgpu_scene_frame_buffer, publish_gpgpu_video_frame_buffer,
    publish_gpu_font_frame_buffer, publish_gpu_frame_buffer, published_rgba_view,
    release_published_frame, retain_published_frame, writable_rgba_view,
};
pub(crate) use gpgpu_preview_consumer::{
    GPGPU_PREVIEW_DEFAULT_CADENCE_MS, GPGPU_PREVIEW_DEFAULT_PUBLISH_EVERY, GpgpuPreviewMetrics,
    GpgpuPreviewPreset, gpgpu_preview_consumer_service_task, gpgpu_preview_status,
    request_cpp_font_preview_start, request_cpp_font_rush_start, request_cpp_font_rush_stop,
    request_cpp_gallery_start, request_gpgpu_preview_stop,
};
pub(crate) use gpgpu_svg_probe_consumer::{
    GpgpuSvgProbeConfig, gpgpu_svg_probe_consumer_service_task, gpgpu_svg_probe_status,
    request_gpgpu_svg_probe_start, request_gpgpu_svg_probe_stop,
};
#[cfg(feature = "trueos_h264_encode_stream")]
pub(crate) use h264_encode_stream::{ui4_h264_encode_prepare_task, ui4_h264_encode_stream_task};
#[cfg(feature = "trueos_h264_encode_stream")]
pub(crate) use h264_encode_udp::ui4_h264_encode_udp_egress_task;
pub(crate) use input_broker::{
    Ui4ButtonPhase, Ui4InputEvent, Ui4KeyboardEvent, Ui4PanEvent, Ui4PanPhase, Ui4VisualRect,
    focused_keyboard_state, reselect_window_for_cursor, select_window_for_cursor,
    show_context_menu, software_cursor_visuals, take_owner_input_events, ui4_input_service_task,
    window_input_routes,
};
pub(crate) use screenshot::{
    COMPACT_WINDOW_GRID_EXTENT, COMPACT_WINDOW_OBSERVATION_MAX_PNG_BYTES,
    capture_compact_window_observation, ui4_screenshot_service_task,
};
pub(crate) use slot4_service::ui4_slot4_service_task;
pub(crate) use video_frame::{
    DecodedNv12Source, DecodedVideoConversionProbeReport, DecodedVideoConversionReport,
    VIDEO_RGBA_BUFFER_COUNT, begin_decoded_nv12_conversion_batch, begin_shell_decoded_video_player,
    enqueue_decoded_nv12_stream_frame, stop_decoded_nv12_stream, ui4_video_conversion_service_task,
    wait_decoded_nv12_conversion_idle,
};

pub(crate) use window_broker::{
    WindowBrokerError, WindowCreate, WindowId,
    WindowInteraction, WindowOwner, WindowPlacement, WindowPlane, WindowSessionCloseRequest,
    WindowSessionId, WindowSnapshot, WindowState, acknowledge_window_frame,
    advance_window_close_transitions, application_windows_for_output_with_revision,
    begin_additional_window_session, begin_window_session, close_window,
    commit_window_frame_replacement, create_window, finish_window_session,
    finish_window_session_with_request, move_window,
    publish_window_frame, publish_window_frames, replace_window_frame, retire_frame_when_released,
    set_window_hit_testable, set_window_placement, set_windows_visible,
    take_window_first_presentation, toggle_window_maximized,
    ui4_window_broker_snapshot_service_task, visible_windows_for_output,
    wait_for_window_composition_change,
    wait_for_window_first_presentation, window_close_transitions_active,
    window_composition_revision, window_placement,
};

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct OwnerReleaseSummary {
    pub(crate) surfaces: usize,
    pub(crate) input_routes: usize,
    pub(crate) input_events: usize,
    pub(crate) context_menus: usize,
}

/// One fresh, producer-resource view of UI4.
///
/// The compositor service and its transparent parking surfaces are permanent
/// infrastructure and are intentionally not counted. A frame remains active
/// until its frame-pool allocation is destroyed, even after its broker window
/// has closed, so callers can distinguish a visually empty broker from a fully
/// retired UI4 producer set.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Ui4LiveResourceUsage {
    pub(crate) active_frames: usize,
    pub(crate) active_sessions: usize,
    pub(crate) live_windows: usize,
}

impl Ui4LiveResourceUsage {
    /// No producer can currently reach a display plane through the broker.
    ///
    /// Detached frame allocations are intentionally allowed. Gridpaper keeps
    /// one retained, offscreen frame warm at boot even when it has no session
    /// or window, so a detached allocation alone does not make the UI4 output
    /// busy.
    pub(crate) const fn is_display_idle(self) -> bool {
        self.active_sessions == 0 && self.live_windows == 0
    }

    pub(crate) const fn is_fully_retired(self) -> bool {
        self.active_frames == 0 && self.is_display_idle()
    }
}

/// Read live ownership state rather than the low-frequency diagnostic watch.
///
/// This is an observation, not a reservation: a producer can begin a session
/// after the function returns.
pub(crate) fn ui4_live_resource_usage() -> Ui4LiveResourceUsage {
    let (active_sessions, live_windows) = window_broker::live_resource_counts();
    Ui4LiveResourceUsage {
        active_frames: frame_pool::active_frame_count(),
        active_sessions,
        live_windows,
    }
}

/// Release all UI4 resources belonging to an application owner.
///
/// UI4 deliberately does not infer owner liveness. The application lifecycle
/// calls this operation when an owner ceases to exist.
pub(crate) fn release_owner_resources(owner: WindowOwner) -> OwnerReleaseSummary {
    let surfaces = blueprint_text::release_owner_resources(owner);
    let (input_routes, input_events) = input_broker::release_owner(owner);
    let context_menus = context_menu::release_owner(owner);
    cursor_frame_inout::owner_closed(owner);
    OwnerReleaseSummary {
        surfaces,
        input_routes,
        input_events,
        context_menus,
    }
}

pub(crate) const OUTPUT_COUNT: usize = 4;
pub(crate) const UNIVERSAL_PLANE_COUNT: usize = 5;
/// Default broker extent for kernel UI4 producers that do not yet negotiate a
/// size with an external application.
pub(crate) const DEFAULT_FRAME_WIDTH: u32 = 768;
pub(crate) const DEFAULT_FRAME_HEIGHT: u32 = 512;
/// Temporary decoded-video allocation ceiling. Video producers may choose any
/// aspect ratio, but one broker Frame may not exceed a 2560x1440 pixel budget.
pub(crate) const VIDEO_FRAME_MAX_PIXELS: u64 = 2_560 * 1_440;
pub(crate) const PRIMARY_PLANE_SLOT: usize = 0;
pub(crate) const ALPHA_OVERLAY_PLANE_SLOT: usize = 1;
pub(crate) const RGB_OVERLAY_PLANE_SLOT_2: usize = 2;
pub(crate) const RGB_OVERLAY_PLANE_SLOT_3: usize = 3;
/// Highest universal plane. UI4 reserves it for input chrome and the trusted
/// kernel color picker; neither participates in an application composition
/// surface.
pub(crate) const INTERACTION_OVERLAY_PLANE_SLOT: usize = 4;
// Compatibility aliases for the parked linked-NV12 display-plane experiment.
// Normal UI4 video is converted by the GuC into an ordinary streaming RGBA
// Frame on slot 1; decoder planes are never assigned to either of these roles.
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

/// Hardware-neutral capabilities published by the display owner for one UI4
/// logical output.
///
/// `application_plane_mask` uses UI4's stable pipe-local plane slot numbers.
/// Reserved interaction/cursor carriers are deliberately absent, even when the
/// physical display engine exposes them.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct Ui4OutputCapabilities {
    pub(crate) output: OutputId,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) application_plane_mask: u8,
}

impl Ui4OutputCapabilities {
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) const fn application_plane_count(self) -> usize {
        self.application_plane_mask.count_ones() as usize
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) const fn supports_application_plane(self, slot: usize) -> bool {
        slot < u8::BITS as usize && self.application_plane_mask & (1u8 << slot) != 0
    }
}

const UI4_APPLICATION_PLANE_MASK: u8 = (1u8 << INTERACTION_OVERLAY_PLANE_SLOT) - 1;
static UI4_OUTPUT_CAPABILITIES: spin::Mutex<[Option<Ui4OutputCapabilities>; OUTPUT_COUNT]> =
    spin::Mutex::new([None; OUTPUT_COUNT]);

/// Publish one immutable display-owner capability descriptor into UI4.
///
/// The display driver calls this only after it has proved the complete plane
/// stack live. Repeating the same proof is idempotent; a conflicting descriptor
/// fails closed instead of silently changing the meaning of live plane slots.
pub(crate) fn publish_ui4_output_capabilities(
    capabilities: Ui4OutputCapabilities,
) -> Result<(), &'static str> {
    publish_output_capabilities_into(&mut UI4_OUTPUT_CAPABILITIES.lock(), capabilities)
}

fn publish_output_capabilities_into(
    outputs: &mut [Option<Ui4OutputCapabilities>; OUTPUT_COUNT],
    capabilities: Ui4OutputCapabilities,
) -> Result<(), &'static str> {
    if capabilities.width == 0 || capabilities.height == 0 {
        return Err("ui4-output-capabilities-empty-extent");
    }
    if capabilities.application_plane_mask == 0
        || capabilities.application_plane_mask & !UI4_APPLICATION_PLANE_MASK != 0
    {
        return Err("ui4-output-capabilities-invalid-application-plane-mask");
    }
    let slot = capabilities.output.slot();
    match outputs[slot] {
        None => {
            outputs[slot] = Some(capabilities);
            Ok(())
        }
        Some(current) if current == capabilities => Ok(()),
        Some(_) => Err("ui4-output-capabilities-conflict"),
    }
}

pub(crate) fn ui4_output_capabilities(output: OutputId) -> Option<Ui4OutputCapabilities> {
    UI4_OUTPUT_CAPABILITIES.lock()[output.slot()]
}

/// The producer which will write a frame. This does not imply update cadence.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum FrameContent {
    Video,
    Image,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    CpuBlit,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u8)]
pub(crate) enum FrameBuffering {
    Single = 1,
    Double = 2,
    Triple = 3,
    Quad = 4,
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
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    Xbgr8888,
    /// Per-pixel alpha overlay storage; RGB is already multiplied by alpha.
    Rgba8888Premultiplied,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) enum Nv12Layout {
    Linear,
    YTiled,
}

/// One trusted native NV12 render source. This type carries no plane
/// assignment: the GuC conversion consumes it while rebuilding the primary
/// XRGB swap surface.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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
    /// Explicit producer/display ownership depth. Cadence describes when
    /// pixels change; it does not silently choose how many allocations exist.
    pub(crate) buffering: FrameBuffering,
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
    pub(crate) cadence: FrameCadence,
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
    VideoRequiresStreamingCadence,
    VideoRequiresQuadBuffering,
    VideoExceedsPixelSoftCap,
    RenderSceneRequiresPremultipliedRgba,
    RenderSceneRequiresStreamingCadence,
    RenderSceneRequiresTripleBuffering,
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
            (FrameContent::RenderScene3d, ScanoutFormat::Rgba8888Premultiplied) => {}
            (FrameContent::RenderScene3d, _) => {
                return Err(FramePlanError::RenderSceneRequiresPremultipliedRgba);
            }
            _ => {}
        }
        if let FrameContent::Video = spec.content {
            if !matches!(spec.cadence, FrameCadence::Streaming) {
                return Err(FramePlanError::VideoRequiresStreamingCadence);
            }
            if !matches!(spec.buffering, FrameBuffering::Quad) {
                return Err(FramePlanError::VideoRequiresQuadBuffering);
            }
            if !video_frame_extent_admitted(spec.width, spec.height) {
                return Err(FramePlanError::VideoExceedsPixelSoftCap);
            }
        }
        if let FrameContent::RenderScene3d = spec.content {
            if !matches!(spec.cadence, FrameCadence::Streaming) {
                return Err(FramePlanError::RenderSceneRequiresStreamingCadence);
            }
            if !matches!(spec.buffering, FrameBuffering::Triple) {
                return Err(FramePlanError::RenderSceneRequiresTripleBuffering);
            }
        }
        Ok(Self {
            output: spec.output,
            content: spec.content,
            cadence: spec.cadence,
            format: spec.format,
            alpha: spec.format.alpha(),
            plane: spec.format.plane(),
            buffering: spec.buffering,
            width: spec.width,
            height: spec.height,
            base_color: spec.base_color,
        })
    }
}

/// Frames which may intentionally share their requested broker plane. A lone
/// member still takes the direct-scanout path when eligible; two or more
/// members are composed together by UI4 instead of consuming one hardware
/// plane each. Dirty FontScene frames retain double buffering, while streaming
/// RenderScene frames retain triple buffering, so composition always reads a
/// stable published front.
pub(crate) const fn frame_plan_shares_compositor_plane(plan: FramePlan) -> bool {
    matches!(plan.buffering, FrameBuffering::Single)
        || matches!(
            (plan.content, plan.cadence, plan.buffering),
            (FrameContent::FontScene2d, FrameCadence::Dirty, FrameBuffering::Double)
                | (FrameContent::RenderScene3d, FrameCadence::Streaming, FrameBuffering::Triple)
        )
}

pub(crate) const fn video_frame_extent_admitted(width: u32, height: u32) -> bool {
    width != 0
        && height != 0
        && (width as u64).saturating_mul(height as u64) <= VIDEO_FRAME_MAX_PIXELS
}

const _: () = {
    assert!(FrameBuffering::Single.count() == 1);
    assert!(FrameBuffering::Double.count() == 2);
    assert!(FrameBuffering::Triple.count() == 3);
    assert!(FrameBuffering::Quad.count() == 4);
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
    let admitted_video = FrameSpec {
        output: OutputId::from_slot(0).unwrap(),
        content: FrameContent::Video,
        cadence: FrameCadence::Streaming,
        buffering: FrameBuffering::Quad,
        format: ScanoutFormat::Rgba8888Premultiplied,
        width: DEFAULT_FRAME_WIDTH,
        height: DEFAULT_FRAME_HEIGHT,
        base_color: None,
    };
    assert!(matches!(FramePlan::from_spec(admitted_video), Ok(_)));
    assert!(matches!(
        FramePlan::from_spec(FrameSpec {
            cadence: FrameCadence::Dirty,
            ..admitted_video
        }),
        Err(FramePlanError::VideoRequiresStreamingCadence)
    ));
    assert!(matches!(
        FramePlan::from_spec(FrameSpec {
            buffering: FrameBuffering::Double,
            ..admitted_video
        }),
        Err(FramePlanError::VideoRequiresQuadBuffering)
    ));
    assert!(matches!(
        FramePlan::from_spec(FrameSpec {
            width: 2_560,
            height: 1_440,
            ..admitted_video
        }),
        Ok(_)
    ));
    assert!(matches!(
        FramePlan::from_spec(FrameSpec {
            width: 2_560,
            height: 1_441,
            ..admitted_video
        }),
        Err(FramePlanError::VideoExceedsPixelSoftCap)
    ));
    let admitted_resident_scene = FrameSpec {
        content: FrameContent::RenderScene3d,
        cadence: FrameCadence::Streaming,
        buffering: FrameBuffering::Triple,
        ..admitted_video
    };
    assert!(matches!(FramePlan::from_spec(admitted_resident_scene), Ok(_)));
    assert!(matches!(
        FramePlan::from_spec(FrameSpec {
            buffering: FrameBuffering::Double,
            ..admitted_resident_scene
        }),
        Err(FramePlanError::RenderSceneRequiresTripleBuffering)
    ));
    let shared_font_scene = match FramePlan::from_spec(FrameSpec {
        content: FrameContent::FontScene2d,
        cadence: FrameCadence::Dirty,
        buffering: FrameBuffering::Double,
        ..admitted_video
    }) {
        Ok(plan) => plan,
        Err(_) => panic!("dirty/double FontScene plan must be valid"),
    };
    assert!(frame_plan_shares_compositor_plane(shared_font_scene));
    let shared_resident_scene = match FramePlan::from_spec(admitted_resident_scene) {
        Ok(plan) => plan,
        Err(_) => panic!("streaming/triple RenderScene plan must be valid"),
    };
    assert!(frame_plan_shares_compositor_plane(shared_resident_scene));
    let isolated_image = match FramePlan::from_spec(FrameSpec {
        content: FrameContent::Image,
        cadence: FrameCadence::Dirty,
        buffering: FrameBuffering::Double,
        ..admitted_video
    }) {
        Ok(plan) => plan,
        Err(_) => panic!("dirty/double image plan must be valid"),
    };
    assert!(!frame_plan_shares_compositor_plane(isolated_image));
};

#[cfg(test)]
mod live_resource_and_output_capability_tests {
    use super::*;

    #[test]
    fn live_resource_usage_distinguishes_display_idle_from_fully_retired() {
        let empty = Ui4LiveResourceUsage::default();
        assert!(empty.is_display_idle());
        assert!(empty.is_fully_retired());

        let detached_frame = Ui4LiveResourceUsage {
            active_frames: 1,
            ..empty
        };
        assert!(detached_frame.is_display_idle());
        assert!(!detached_frame.is_fully_retired());

        for usage in [
            Ui4LiveResourceUsage {
                active_sessions: 1,
                ..empty
            },
            Ui4LiveResourceUsage {
                live_windows: 1,
                ..empty
            },
        ] {
            assert!(!usage.is_display_idle());
            assert!(!usage.is_fully_retired());
        }
    }

    #[test]
    fn output_capabilities_count_and_test_sparse_application_planes() {
        let capabilities = Ui4OutputCapabilities {
            output: OutputId::from_slot(0).unwrap(),
            width: 2_560,
            height: 1_440,
            application_plane_mask: 0b1011,
        };
        assert_eq!(capabilities.application_plane_count(), 3);
        assert!(capabilities.supports_application_plane(0));
        assert!(capabilities.supports_application_plane(1));
        assert!(!capabilities.supports_application_plane(2));
        assert!(capabilities.supports_application_plane(3));
        assert!(!capabilities.supports_application_plane(4));
        assert!(!capabilities.supports_application_plane(usize::MAX));
    }

    #[test]
    fn capability_publication_is_idempotent_and_rejects_invalid_or_conflicting_state() {
        let output = OutputId::from_slot(0).unwrap();
        let capabilities = Ui4OutputCapabilities {
            output,
            width: 2_560,
            height: 1_440,
            application_plane_mask: UI4_APPLICATION_PLANE_MASK,
        };
        let mut outputs = [None; OUTPUT_COUNT];

        assert_eq!(publish_output_capabilities_into(&mut outputs, capabilities), Ok(()));
        assert_eq!(outputs[0], Some(capabilities));
        assert_eq!(publish_output_capabilities_into(&mut outputs, capabilities), Ok(()));
        assert_eq!(
            publish_output_capabilities_into(
                &mut outputs,
                Ui4OutputCapabilities {
                    width: 1_920,
                    ..capabilities
                },
            ),
            Err("ui4-output-capabilities-conflict")
        );

        let mut empty = [None; OUTPUT_COUNT];
        assert_eq!(
            publish_output_capabilities_into(
                &mut empty,
                Ui4OutputCapabilities {
                    width: 0,
                    ..capabilities
                },
            ),
            Err("ui4-output-capabilities-empty-extent")
        );
        assert_eq!(
            publish_output_capabilities_into(
                &mut empty,
                Ui4OutputCapabilities {
                    application_plane_mask: 1 << INTERACTION_OVERLAY_PLANE_SLOT,
                    ..capabilities
                },
            ),
            Err("ui4-output-capabilities-invalid-application-plane-mask")
        );
    }
}
