//! Blueprint transport for VM-owned UI4 scene frames.
//!
//! The kernel derives window ownership from the active Blueprint VM. Callers
//! provide scene operations and placement only; frame handles, writable
//! surfaces, and GPU addresses never cross the ABI boundary. Solara's text
//! producer was the first consumer; shaded scene producers share the same
//! coherent UI4 frame lifecycle.

use alloc::{collections::VecDeque, string::String, vec::Vec};
use spin::Mutex;
use trueos_time::Instant;

use crate::intel::gpgpu::{
    ALPHA_BLEND_WORKLIST_FLAG_COPY, ALPHA_BLEND_WORKLIST_FLAG_SRC_OVER,
    ALPHA_BLEND_WORKLIST_FLAG_TINT_ALPHA, ALPHA_BLEND_WORKLIST_FLAG_TINT_RGB,
    GpgpuAlphaBlendWorklistDesc, GpgpuGlyphMaskLayer, GpgpuOwnedParticleCraftState,
    GpgpuOwnedRgba8Surface, GpgpuPoint, GpgpuRect, GpgpuRgb565Surface, GpgpuRgba8ReleaseFence,
    GpgpuRgba8Surface, GpgpuSpriteQuadWorklistDesc, GpgpuSpriteQuadWorklistRun,
    ParticleCraftParamsV1, SHADERTOY_PARAMS_VERSION, SPRITE_QUAD_WORKLIST_FLAG_PREMUL_SRC,
    SPRITE_QUAD_WORKLIST_FLAG_SRC_OVER, ShaderToyFrameParams, SkyboxSampleRgb565Params,
    Ui4CompositorCompletion, Ui4CompositorSubmission, Ui4CompositorSubmitError,
    Ui4SpriteSceneCompletion, allocate_font_instance_rgba8_surface_cleared,
    alpha_blend_worklist_max_descs, glyph_mask_layers_rgba8_2d_mode, particle_craft_rgba8_frame,
    poll_ui4_blueprint_sprite_scene, poll_ui4_compositor_submission,
    queue_ui4_blueprint_alpha_rects, queue_ui4_blueprint_sprite_scene,
    release_rgba8_surface_for_scanout, shadertoy_rgba8_surface_full, skybox_sample_rgb565_to_rgba8,
    sprite_quad_worklist_max_descs,
};
use crate::intel::gpu_font::{
    GpuFontFace, GpuFontJob, GpuFontJobEntry, GpuFontRgba, GpuFontTextRequest,
    MAX_DYNAMIC_TEXT_CHARS, ensure_font_face_available, recycle_font_job_readback,
    render_font_job_readback_once,
};
use crate::r::font_kernel_service::{
    FontKernelError, FontKernelRetainedScene, FontStampFit, FontStampLayer, FontStampRequest,
    FontStampedBuffer, PendingFontFrameStamp, PendingFontStamp, PendingRetainScene,
    RetainSceneRequest, RetainedFontPositioning, RetainedFontRun, submit_frame_stamp,
    submit_retain_scene, submit_stamp,
};

use super::{
    DamageRect, FrameCadence, FrameContent, FrameHandle, FramePoolError, FrameSpec,
    FrameWriteLease, OutputId, PremultipliedRgba8, ScanoutFormat, Ui4CursorIcon, Ui4CursorSource,
    Ui4InputEvent, WindowCreate, WindowId, WindowOwner, WindowPlacement, WindowPlane,
    WindowSessionCloseRequest, WindowSessionId, acquire_frame_buffer,
    begin_additional_window_session, cancel_frame_buffer, commit_window_frame_replacement,
    create_frame, create_gpu_full_overwrite_frame, create_window, destroy_frame,
    finish_window_session, finish_window_session_with_request, focused_keyboard_state,
    gpgpu_rgba_surface, mark_frame_buffer_cpu_authored, mark_frame_buffer_fully_opaque,
    publish_frame_buffer, publish_gpgpu_scene_frame_buffer, publish_resident_scene_frame_buffer,
    publish_window_frame, replace_window_frame, set_window_cursor_icon, set_window_custom_cursor,
    set_window_hit_testable, set_window_placement, take_owner_input_events,
    take_window_first_presentation, window_input_routes, window_placement, writable_rgba_view,
};

const MAX_SURFACES: usize = 32;
const MAX_FRAME_WIDTH: u32 = 2_560;
const MAX_FRAME_HEIGHT: u32 = 1_440;
const MAX_TEXT_ROWS: usize = 64;
const MAX_FONT_CANVAS_ROWS: usize = 256;
const MAX_FONT_CANVAS_RUNS_PER_LAYER: usize = 64;
const MAX_FONT_CANVAS_INTERNAL_LAYERS: usize = 64;
const MAX_TEXT_ROW_BYTES: usize = 1_024;
const MAX_NATIVE_FONT_SIZES: usize = 32;
const MAX_PENDING_POINTER_EVENTS: usize = 256;
const MAX_PENDING_PAN_EVENTS: usize = 256;
const MAX_PENDING_KEYBOARD_EVENTS: usize = 256;
const MAX_INPUT_ROUTES: usize = 32;
const IMAGE_SOURCE_READ_CHUNK_BYTES: usize = 16 * 1024;
const IMAGE_SOURCE_FORMAT_JPEG: u32 = 1;
const IMAGE_SOURCE_FORMAT_RGBA8: u32 = 2;
const RETAINED_TEXT_MASK_BATCH_CAPACITY: usize = 64;
const TEXT_ROWS_WIRE_HEADER_BYTES: usize = 16;
const TEXT_ROW_WIRE_HEADER_BYTES: usize = 12;
const TEXT_SCENE_WIRE_HEADER_BYTES: usize = 16;
const TEXT_SCENE_ROW_WIRE_HEADER_BYTES: usize = 16;
const FONT_CANVAS_WIRE_HEADER_BYTES: usize = 12;
const FONT_CANVAS_ROW_WIRE_HEADER_BYTES: usize = 20;
const UI4_SCENE_SOURCE_GPU: u64 = 0x3000_0000;
const UI4_SCENE_SOURCE_MAX_BYTES: usize = 128 * 1024 * 1024;
const UI4_SCENE_SPRITE_GPU: u64 = UI4_SCENE_SOURCE_GPU + UI4_SCENE_SOURCE_MAX_BYTES as u64;
const UI4_SCENE_SPRITE_MAX_BYTES: usize = 128 * 1024 * 1024;
const UI4_SCENE_SOLID_SOURCE_BYTES: usize = 4096;
const MAX_SPRITE_QUADS: usize = 8_192;
const UI4_SPRITE_BATCH_TIMEOUT_NS: u64 = 1_000_000_000;
const SPRITE_QUAD_FLAG_SRC_OVER: u32 = 1 << 0;
const SPRITE_QUAD_VALID_FLAGS: u32 = SPRITE_QUAD_FLAG_SRC_OVER;
const _: () = {
    assert!(UI4_SCENE_SOURCE_GPU.is_multiple_of(4096));
    assert!(UI4_SCENE_SPRITE_GPU.is_multiple_of(4096));
    assert!(
        UI4_SCENE_SPRITE_GPU + UI4_SCENE_SPRITE_MAX_BYTES as u64
            <= crate::intel::gpgpu::DIRECT_RCS_PPGTT_LIMIT_BYTES
    );
};

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct BlueprintImageSourceInfo {
    pub format: u32,
    pub width: u32,
    pub height: u32,
    pub byte_len: u32,
}

const IMAGE_SOURCE_FORMAT_PNG: u32 = 3;
const INTEL_GRAPHICS_LOGO_PNG: &[u8] = include_bytes!("../../Intel_Graphics_logo.png");

pub(crate) fn blueprint_image_source_info(name: &str) -> Result<BlueprintImageSourceInfo, i32> {
    match name {
        "kernel:logo" => Ok(BlueprintImageSourceInfo {
            format: IMAGE_SOURCE_FORMAT_JPEG,
            width: 0,
            height: 0,
            byte_len: crate::virtio_gpu_logo::embedded_logo_jpeg().len() as u32,
        }),
        "kernel:bgrt" => {
            let Some((width, height, pixels)) = crate::efi::acpi::bgrt::decoded_logo_rgba() else {
                return Err(ERROR_NOT_FOUND);
            };
            let byte_len = pixels.len().checked_mul(4).ok_or(ERROR_INVALID)?;
            Ok(BlueprintImageSourceInfo {
                format: IMAGE_SOURCE_FORMAT_RGBA8,
                width: width as u32,
                height: height as u32,
                byte_len: byte_len as u32,
            })
        }
        "kernel:intel-graphics" => Ok(BlueprintImageSourceInfo {
            format: IMAGE_SOURCE_FORMAT_PNG,
            width: 565,
            height: 565,
            byte_len: INTEL_GRAPHICS_LOGO_PNG.len() as u32,
        }),
        _ => Err(ERROR_NOT_FOUND),
    }
}

pub(crate) fn copy_blueprint_image_source(
    name: &str,
    offset: usize,
    out: &mut [u8],
) -> Result<usize, i32> {
    if out.len() > IMAGE_SOURCE_READ_CHUNK_BYTES {
        return Err(ERROR_INVALID);
    }
    match name {
        "kernel:logo" => {
            let source = crate::virtio_gpu_logo::embedded_logo_jpeg();
            let available = source.get(offset..).ok_or(ERROR_INVALID)?;
            let copied = available.len().min(out.len());
            out[..copied].copy_from_slice(&available[..copied]);
            Ok(copied)
        }
        "kernel:bgrt" => {
            let Some((_, _, pixels)) = crate::efi::acpi::bgrt::decoded_logo_rgba() else {
                return Err(ERROR_NOT_FOUND);
            };
            let total = pixels.len().checked_mul(4).ok_or(ERROR_INVALID)?;
            if offset > total {
                return Err(ERROR_INVALID);
            }
            let copied = (total - offset).min(out.len());
            for (relative, byte) in out[..copied].iter_mut().enumerate() {
                let pixel = pixels[(offset + relative) / 4];
                *byte = match (offset + relative) % 4 {
                    0 => (pixel >> 16) as u8,
                    1 => (pixel >> 8) as u8,
                    2 => pixel as u8,
                    _ => u8::MAX,
                };
            }
            Ok(copied)
        }
        "kernel:intel-graphics" => {
            let available = INTEL_GRAPHICS_LOGO_PNG.get(offset..).ok_or(ERROR_INVALID)?;
            let copied = available.len().min(out.len());
            out[..copied].copy_from_slice(&available[..copied]);
            Ok(copied)
        }
        _ => Err(ERROR_NOT_FOUND),
    }
}

/// Longest UTF-8 label one context-menu row may carry. UI4 renders a fixed
/// number of characters per row; this bound keeps the wire record small while
/// leaving room for multi-byte scalars.
const MAX_CONTEXT_MENU_LABEL_BYTES: usize = 64;
/// Cursor identity plus entry count.
const CONTEXT_MENU_WIRE_HEADER_BYTES: usize = 20;
/// Action id, enabled flag, and label length per entry.
const CONTEXT_MENU_ENTRY_WIRE_HEADER_BYTES: usize = 12;

const ERROR_INVALID: i32 = -1;
const ERROR_CONTEXT: i32 = -2;
const ERROR_NOT_FOUND: i32 = -3;
const ERROR_STATE: i32 = -4;
const ERROR_FONT: i32 = -5;
const ERROR_UI4: i32 = -6;
pub(crate) const ERROR_BUSY: i32 = -7;
const GUEST_TEXT_SCENE_BUSY_POLL_MS: u64 = 2;
const TEXT_SCENE_FONT_ID_STAMP_ONCE: u32 = 1 << 31;
const TEXT_SCENE_FONT_ID_BACKBUFFER: u32 = 1 << 30;
const TEXT_SCENE_FONT_ID_FLAGS: u32 = TEXT_SCENE_FONT_ID_STAMP_ONCE | TEXT_SCENE_FONT_ID_BACKBUFFER;
const TEXT_BACKBUFFER_SPRITE_ID: u32 = u32::MAX;
const TEXT_BACKBUFFER_MAX_EXTENT: u32 = 4_096;
const TEXT_BACKBUFFER_MAX_GLYPHS: usize = 4_096;
const CLOSE_PERSIST_FINAL_FRAME: u32 = 1 << 0;
const CLOSE_VALID_FLAGS: u32 = CLOSE_PERSIST_FINAL_FRAME;
pub const UI4_VISUAL_SOFT_CAP_HZ: u32 = 60;

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct TrueosUi4SolaraFontSize {
    pub native_scale: u32,
    pub target_pixels: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct TrueosUi4SolaraTextRow {
    pub text_ptr: *const u8,
    pub text_len: usize,
    pub x: f32,
    pub y: f32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct TrueosUi4SolaraSceneTextRow {
    pub text_ptr: *const u8,
    pub text_len: usize,
    pub x: f32,
    pub y: f32,
    pub font_pixels: f32,
}

/// One colored row in a persistent, transparent RGBA8 font canvas.
///
/// The consumer submits all rows together. FontKernel may group equal colors
/// internally, but the only externally visible result is one owned RGBA8
/// surface retained with the UI4 window.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct TrueosUi4FontCanvasRow {
    pub text_ptr: *const u8,
    pub text_len: usize,
    pub x: f32,
    pub y: f32,
    pub font_pixels: f32,
    pub color_rgba: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct TrueosUi4PanEvent {
    pub controller_id: u32,
    pub slot_id: u32,
    pub ep_target: u32,
    pub hid_kind: u32,
    pub phase: u32,
    pub x: u32,
    pub y: u32,
    pub local_x: i32,
    pub local_y: i32,
    pub dx: i32,
    pub dy: i32,
    pub combo_id: u32,
    pub vcursor: u32,
}

const _: () = assert!(core::mem::size_of::<TrueosUi4PanEvent>() == 13 * 4);

/// One broker-requested final frame extent. UI4 may animate the already-live
/// surface toward this geometry in its presentation plane, but never emits
/// intermediate animation sizes to the Blueprint. The Blueprint may ignore
/// the event or replace its allocation through
/// `trueos_cabi_ui4_scene_frame_resize` exactly once.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct TrueosUi4ResizeEvent {
    pub old_width: u32,
    pub old_height: u32,
    pub width: u32,
    pub height: u32,
}

const _: () = assert!(core::mem::size_of::<TrueosUi4ResizeEvent>() == 4 * 4);

/// Stable identity for one of the kernel's N independent cursor routes.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct TrueosUi4CursorSource {
    pub controller_id: u32,
    pub slot_id: u32,
    pub ep_target: u32,
    pub hid_kind: u32,
}

const _: () = assert!(core::mem::size_of::<TrueosUi4CursorSource>() == 4 * 4);

/// One row of a Blueprint-requested context menu. `enabled` is zero for a
/// greyed label which reports no action, non-zero for a selectable row.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct TrueosUi4ContextMenuEntry {
    pub label_ptr: *const u8,
    pub label_len: usize,
    pub action_id: u32,
    pub enabled: u32,
}

/// Outcome of one context-menu invocation. `selected` is non-zero only when
/// the user chose an enabled row, in which case `action_id` is that row's id.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct TrueosUi4ContextMenuEvent {
    pub context: u64,
    pub action_id: u32,
    pub selected: u32,
    pub reason: u32,
}

/// One selected-frame pointer event after UI4 hit testing and capture.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct TrueosUi4PointerEvent {
    pub controller_id: u32,
    pub slot_id: u32,
    pub ep_target: u32,
    pub hid_kind: u32,
    pub x: u32,
    pub y: u32,
    pub local_x: i32,
    pub local_y: i32,
    pub dx: i32,
    pub dy: i32,
    pub wheel: i32,
    pub buttons_down: u32,
    pub buttons_pressed: u32,
    pub buttons_released: u32,
    pub combo_id: u32,
    pub vcursor: u32,
}

const _: () = assert!(core::mem::size_of::<TrueosUi4PointerEvent>() == 16 * 4);

/// Held-key snapshot for the keyboard assigned to this selected window's
/// cursor/HUT route. HID usages index `key_down_bits` directly.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct TrueosUi4KeyboardState {
    pub controller_id: u32,
    pub slot_id: u32,
    pub ep_target: u32,
    pub combo_id: u32,
    pub modifiers: u8,
    pub source_kind: u8,
    pub virtual_keyboard: u8,
    pub reserved0: u8,
    pub keys: [u8; 6],
    pub ascii: [u8; 6],
    pub key_down_bits: [u32; 8],
}

const _: () = assert!(core::mem::size_of::<TrueosUi4KeyboardState>() == 16 * 4);

pub const UI4_INPUT_ROUTE_SELECTED_FOR_WINDOW: u32 = 1 << 0;
pub const UI4_INPUT_ROUTE_APP_FOCUS: u32 = 1 << 1;
pub const UI4_INPUT_ROUTE_VCURSOR: u32 = 1 << 2;
pub const UI4_INPUT_ROUTE_KEYBOARD_PRESENT: u32 = 1 << 3;

/// One cursor/combo route known to UI4, including the held keyboard state
/// paired by HUT when present. A Blueprint can retain route identity after a
/// player joins and detect that exact player's selection leaving its window.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct TrueosUi4InputRouteState {
    pub cursor_controller_id: u32,
    pub cursor_slot_id: u32,
    pub cursor_ep_target: u32,
    pub cursor_hid_kind: u32,
    pub combo_id: u32,
    pub color_rgba: u32,
    pub flags: u32,
    pub keyboard_controller_id: u32,
    pub keyboard_slot_id: u32,
    pub keyboard_ep_target: u32,
    pub keyboard_modifiers: u8,
    pub keyboard_source_kind: u8,
    pub virtual_keyboard: u8,
    pub reserved0: u8,
    pub keys: [u8; 6],
    pub ascii: [u8; 6],
    pub key_down_bits: [u32; 8],
}

const _: () = assert!(core::mem::size_of::<TrueosUi4InputRouteState>() == 88);

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct TrueosUi4SkyboxRenderParams {
    pub right_x: f32,
    pub right_y: f32,
    pub right_z: f32,
    pub up_x: f32,
    pub up_y: f32,
    pub up_z: f32,
    pub forward_x: f32,
    pub forward_y: f32,
    pub forward_z: f32,
    pub aspect_tan_half_fov_y: f32,
    pub tan_half_fov_y: f32,
    pub rect_x: u32,
    pub rect_y: u32,
    pub rect_width: u32,
    pub rect_height: u32,
}

const _: () = assert!(core::mem::size_of::<TrueosUi4SkyboxRenderParams>() == 15 * 4);

/// Versioned, pointer-free ParticleCraft control block. GPU addresses and
/// persistent state ownership remain entirely inside the kernel.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct TrueosUi4ParticleCraftParamsV1 {
    pub version: u32,
    pub flags: u32,
    pub seed: u32,
    pub active_count: u32,
    pub dt_seconds: f32,
    pub time_seconds: f32,
    pub emitter_x: f32,
    pub emitter_y: f32,
    pub attractor_x: f32,
    pub attractor_y: f32,
    pub attraction: f32,
    pub swirl: f32,
    pub gravity_x: f32,
    pub gravity_y: f32,
    pub drag: f32,
    pub intensity: f32,
}

const _: () = assert!(core::mem::size_of::<TrueosUi4ParticleCraftParamsV1>() == 16 * 4);

/// Versioned, pointer-free controls for one kernel-reviewed ShaderToy image.
/// `shader_id` selects an immutable catalog entry; source, binaries, and GPU
/// addresses are deliberately absent from this provisional ABI.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct TrueosUi4ShadertoyParamsV1 {
    pub version: u32,
    pub shader_id: u32,
    pub frame: u32,
    pub flags: u32,
    pub time_seconds: f32,
    pub delta_seconds: f32,
    pub frame_rate: f32,
    pub sample_rate: f32,
    pub mouse_x: f32,
    pub mouse_y: f32,
    pub click_x: f32,
    pub click_y: f32,
    pub date_year: f32,
    pub date_month: f32,
    pub date_day: f32,
    pub date_seconds: f32,
}

const _: () = assert!(core::mem::size_of::<TrueosUi4ShadertoyParamsV1>() == 16 * 4);

/// One ordered, straight-alpha RGBA sprite operation. Sprite id zero selects
/// the frame-owned one-pixel white source and therefore represents a solid
/// rectangle when every UV is zero.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct TrueosUi4SpriteQuad {
    pub sprite_id: u32,
    pub c0_x: f32,
    pub c0_y: f32,
    pub c0_u: f32,
    pub c0_v: f32,
    pub c1_x: f32,
    pub c1_y: f32,
    pub c1_u: f32,
    pub c1_v: f32,
    pub c2_x: f32,
    pub c2_y: f32,
    pub c2_u: f32,
    pub c2_v: f32,
    pub c3_x: f32,
    pub c3_y: f32,
    pub c3_u: f32,
    pub c3_v: f32,
    pub color_rgba: u32,
    pub flags: u32,
}

const _: () = assert!(core::mem::size_of::<TrueosUi4SpriteQuad>() == 19 * 4);

struct BlueprintSceneSurface {
    owner: WindowOwner,
    session: WindowSessionId,
    frame: FrameHandle,
    window: WindowId,
    width: u32,
    height: u32,
    cadence: FrameCadence,
    visual_cadence: Option<BlueprintVisualCadence>,
    pending_resize: Option<BlueprintPendingResize>,
    write_lease: Option<FrameWriteLease>,
    pending_gpu_release: Option<GpgpuRgba8ReleaseFence>,
    pending_render_release: Option<crate::intel::render::ResidentSceneReleaseFence>,
    gpu_submission_unretired: bool,
    vgpu_surface: Option<u64>,
    particle_craft: Option<GpgpuOwnedParticleCraftState>,
    placement: WindowPlacement,
    launch_selection: Option<(Ui4CursorSource, u32, u32)>,
    skybox: Option<OwnedRgb565Surface>,
    skybox_upload: Option<Rgb565Upload>,
    sprites: Vec<(u32, OwnedRgba8Surface)>,
    sprite_upload: Option<Rgba8Upload>,
    solid_source: Option<OwnedRgba8Surface>,
    sprite_clear_rgba: Option<u32>,
    sprite_scene_upload: Option<SpriteSceneUpload>,
    pending_pointer_events: VecDeque<TrueosUi4PointerEvent>,
    pending_pan_events: VecDeque<TrueosUi4PanEvent>,
    pending_resize_events: VecDeque<TrueosUi4ResizeEvent>,
    pending_keyboard_events: VecDeque<crate::r::keyboard::TrueosKeyboardOutputEvent>,
    pending_keyboard_burst: VecDeque<crate::r::keyboard::TrueosKeyboardOutputEvent>,
    retained_text_layers: Vec<BlueprintRetainedTextLayer>,
    retained_text_cursor: usize,
    retained_text_rendered: bool,
    retained_text_backbuffer_extent: Option<(u32, u32)>,
    retained_text_backbuffer: Option<GpgpuOwnedRgba8Surface>,
    font_canvas: Option<BlueprintFontCanvas>,
    stamped_text_layers: Vec<BlueprintStampedTextLayer>,
    stamped_text_cursor: usize,
    stamped_text_pending: Option<PendingFontFrameStamp>,
    stamped_text_rendered: bool,
}

#[derive(Copy, Clone, Debug)]
struct BlueprintPendingResize {
    /// The broker continues presenting this frame until `frame` has a
    /// released front and both frame and placement can be committed together.
    previous_frame: FrameHandle,
    placement: WindowPlacement,
}

#[derive(Copy, Clone, Debug)]
struct BlueprintVisualCadence {
    target_hz: u32,
    next_tick: u64,
    remainder: u64,
}

impl BlueprintVisualCadence {
    fn new(target_hz: u32) -> Self {
        Self {
            target_hz,
            next_tick: Instant::now().as_ticks(),
            remainder: 0,
        }
    }

    fn wait_ms(&self) -> u64 {
        let now = Instant::now().as_ticks();
        let remaining_ticks = self.next_tick.saturating_sub(now);
        remaining_ticks
            .saturating_mul(1_000)
            .saturating_add(trueos_time::TICK_HZ.saturating_sub(1))
            / trueos_time::TICK_HZ
    }

    fn consume_admission(&mut self) {
        let now = Instant::now().as_ticks();
        if self.next_tick < now {
            self.next_tick = now;
            self.remainder = 0;
        }
        let hz = self.target_hz as u64;
        let tick_hz = trueos_time::TICK_HZ;
        let mut period = tick_hz / hz;
        self.remainder = self.remainder.saturating_add(tick_hz % hz);
        period = period.saturating_add(self.remainder / hz);
        self.remainder %= hz;
        self.next_tick = self.next_tick.saturating_add(period.max(1));
    }
}

struct BlueprintRetainedTextLayer {
    description: BlueprintRetainedTextDescription,
    color_rgba: u32,
    translation_px: [i32; 2],
    state: BlueprintRetainedTextState,
}

struct BlueprintRetainedTextDescription {
    font: GpuFontFace,
    viewport_width: u32,
    viewport_height: u32,
    runs: Vec<RetainedFontRun>,
}

enum BlueprintRetainedTextState {
    Pending(PendingRetainScene),
    Ready(FontKernelRetainedScene),
    /// The face was available, but none of the row's scalars had an outline.
    /// Keep the logical layer so later rows retain stable cursor indexes.
    NoCoverage,
}

struct BlueprintStampedTextLayer {
    description: BlueprintRetainedTextDescription,
    color_rgba: u32,
}

#[derive(Clone)]
struct BlueprintFontCanvasRow {
    text: String,
    position: [f32; 2],
    font_pixels: f32,
    color_rgba: u32,
}

#[derive(Clone)]
struct BlueprintFontCanvasDescription {
    font: GpuFontFace,
    width: u32,
    height: u32,
    rows: Vec<BlueprintFontCanvasRow>,
}

struct BlueprintFontCanvas {
    description: BlueprintFontCanvasDescription,
    pending: Option<PendingFontStamp>,
    ready: Option<FontStampedBuffer>,
    submitted_ms: u64,
}

impl BlueprintFontCanvasDescription {
    fn same_canvas(&self, next: &Self) -> bool {
        self.font == next.font
            && self.width == next.width
            && self.height == next.height
            && self.rows.len() == next.rows.len()
            && self.rows.iter().zip(next.rows.iter()).all(|(old, new)| {
                old.text == new.text
                    && old.position[0].to_bits() == new.position[0].to_bits()
                    && old.position[1].to_bits() == new.position[1].to_bits()
                    && old.font_pixels.to_bits() == new.font_pixels.to_bits()
                    && old.color_rgba == new.color_rgba
            })
    }
}

impl BlueprintRetainedTextDescription {
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    fn same_scene(&self, next: &Self) -> bool {
        self.font == next.font
            && self.viewport_width == next.viewport_width
            && self.viewport_height == next.viewport_height
            && self.runs.len() == next.runs.len()
            && self.runs.iter().zip(next.runs.iter()).all(|(old, new)| {
                old.text == new.text
                    && old.position[0].to_bits() == new.position[0].to_bits()
                    && old.position[1].to_bits() == new.position[1].to_bits()
                    && old.font_pixels.to_bits() == new.font_pixels.to_bits()
                    && old.slant.to_bits() == new.slant.to_bits()
            })
    }

    /// Return the integral draw-time translation when `next` is the same
    /// retained glyph scene moved as one unit.
    fn translation_to(&self, next: &Self) -> Option<[i32; 2]> {
        if self.font != next.font
            || self.viewport_width != next.viewport_width
            || self.viewport_height != next.viewport_height
            || self.runs.len() != next.runs.len()
        {
            return None;
        }
        let (base, moved) = self.runs.first().zip(next.runs.first())?;
        let delta = [
            moved.position[0] - base.position[0],
            moved.position[1] - base.position[1],
        ];
        if !delta.iter().all(|value| value.is_finite()) {
            return None;
        }
        let integral = [libm::roundf(delta[0]) as i32, libm::roundf(delta[1]) as i32];
        if (delta[0] - integral[0] as f32).abs() > 0.01
            || (delta[1] - integral[1] as f32).abs() > 0.01
        {
            return None;
        }
        let same_scene = self.runs.iter().zip(next.runs.iter()).all(|(old, new)| {
            old.text == new.text
                && old.font_pixels.to_bits() == new.font_pixels.to_bits()
                && old.slant.to_bits() == new.slant.to_bits()
                && ((new.position[0] - old.position[0]) - delta[0]).abs() <= 0.01
                && ((new.position[1] - old.position[1]) - delta[1]).abs() <= 0.01
        });
        same_scene.then_some(integral)
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum BlueprintSurfaceRelease {
    Animated,
    AnimatedAndPersistFinalFrame,
}

#[derive(Copy, Clone)]
struct OwnedRgb565Surface {
    surface: GpgpuRgb565Surface,
    virt: *mut u8,
    bytes: usize,
}

unsafe impl Send for OwnedRgb565Surface {}
unsafe impl Sync for OwnedRgb565Surface {}

struct Rgb565Upload {
    owned: OwnedRgb565Surface,
    packed_len: usize,
    written: usize,
}

#[derive(Copy, Clone)]
struct OwnedRgba8Surface {
    surface: GpgpuRgba8Surface,
    virt: *mut u8,
    bytes: usize,
}

unsafe impl Send for OwnedRgba8Surface {}
unsafe impl Sync for OwnedRgba8Surface {}

struct Rgba8Upload {
    sprite_id: u32,
    owned: OwnedRgba8Surface,
    packed_len: usize,
    written: usize,
}

struct SpriteSceneUpload {
    expected: usize,
    quads: Vec<TrueosUi4SpriteQuad>,
}

static SURFACES: Mutex<Vec<BlueprintSceneSurface>> = Mutex::new(Vec::new());
static RETIRED_FRAMES: Mutex<Vec<FrameHandle>> = Mutex::new(Vec::new());
// An accepted GPU submission whose marker never retired may still reference
// both its destination ring and retained source. Those allocations are never
// recycled into another owner; recovery belongs to a future engine reset.
static QUARANTINED_SURFACES: Mutex<Vec<BlueprintSceneSurface>> = Mutex::new(Vec::new());

/// Revoke every Blueprint scene resource held for an owner.
///
/// The caller owns the application lifecycle decision. UI4 only applies the
/// owner-scoped resource revocation and does not inspect VM state.
pub(crate) fn release_owner_resources(owner: WindowOwner) -> usize {
    let owned = {
        let mut surfaces = SURFACES.lock();
        let mut owned = Vec::new();
        let mut index = 0;
        while index < surfaces.len() {
            if surfaces[index].owner == owner {
                owned.push(surfaces.remove(index));
            } else {
                index += 1;
            }
        }
        owned
    };
    let released = owned.len();
    for surface in owned {
        release_surface(surface, BlueprintSurfaceRelease::Animated);
    }
    released
}

/// Enumerate the kernel font service's native render-target sizes.
///
/// A null output with zero capacity is a size query. The return value is the
/// total number of supported entries, even when the provided output is short.
pub unsafe extern "C" fn trueos_cabi_ui4_solara_font_sizes(
    out: *mut TrueosUi4SolaraFontSize,
    out_cap: usize,
) -> isize {
    if out.is_null() && out_cap != 0 {
        return ERROR_INVALID as isize;
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return unsafe { guest_font_sizes(out, out_cap) };
    }

    let count = crate::intel::render::font_native_scale_count() as usize;
    for (slot, native_scale) in (1..=count as u32).take(out_cap.min(count)).enumerate() {
        let Some(target_pixels) =
            crate::intel::render::font_native_scale_target_pixels(native_scale)
        else {
            return ERROR_FONT as isize;
        };
        // SAFETY: the Blueprint caller supplied capacity for this slot.
        unsafe {
            out.add(slot).write(TrueosUi4SolaraFontSize {
                native_scale,
                target_pixels,
            });
        }
    }
    count as isize
}

/// Create one dirty-cadence UI4 frame and broker window for the active VM.
pub extern "C" fn trueos_cabi_ui4_solara_frame_open(
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> u32 {
    if width == 0 || height == 0 || width > MAX_FRAME_WIDTH || height > MAX_FRAME_HEIGHT {
        return 0;
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, window) = trueos_vm::vmcall::call(
            trueos_vm::vmcall::OP_BP_UI4_SOLARA_FRAME_OPEN,
            pack_i32_pair(x, y),
            pack_u32_pair(width, height),
        );
        return if status == trueos_vm::vmcall::STATUS_OK {
            window as u32
        } else {
            0
        };
    }
    open_blueprint_frame(x, y, width, height, FrameCadence::Dirty, None)
}

/// Create one single-buffered Blueprint snapshot frame for an active VM.
///
/// A published immutable allocation is never written in place. A later
/// `frame_begin` allocates a fresh one-buffer generation privately and
/// `frame_publish` swaps it into the existing window only after it is ready.
pub extern "C" fn trueos_cabi_ui4_scene_frame_open_immutable(
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> u32 {
    if width == 0 || height == 0 || width > MAX_FRAME_WIDTH || height > MAX_FRAME_HEIGHT {
        return 0;
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, window) = trueos_vm::vmcall::call(
            trueos_vm::vmcall::OP_BP_UI4_SCENE_FRAME_OPEN_IMMUTABLE,
            pack_i32_pair(x, y),
            pack_u32_pair(width, height),
        );
        return if status == trueos_vm::vmcall::STATUS_OK {
            window as u32
        } else {
            0
        };
    }
    open_blueprint_frame(x, y, width, height, FrameCadence::Immutable, None)
}

/// Create one streaming/triple-buffered UI4 scene frame for an active VM.
pub extern "C" fn trueos_cabi_ui4_scene_frame_open_streaming(
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> u32 {
    if width == 0 || height == 0 || width > MAX_FRAME_WIDTH || height > MAX_FRAME_HEIGHT {
        return 0;
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, window) = trueos_vm::vmcall::call(
            trueos_vm::vmcall::OP_BP_UI4_SCENE_FRAME_OPEN_STREAMING,
            pack_i32_pair(x, y),
            pack_u32_pair(width, height),
        );
        return if status == trueos_vm::vmcall::STATUS_OK {
            window as u32
        } else {
            0
        };
    }
    open_blueprint_frame(x, y, width, height, FrameCadence::Streaming, None)
}

/// Create a visual-mode UI4 frame. The requested target is brokered by the
/// kernel and cannot exceed the provisional 60 Hz policy ceiling.
pub extern "C" fn trueos_cabi_ui4_scene_frame_open_visual(
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    target_hz: u32,
) -> u32 {
    if width == 0
        || height == 0
        || width > MAX_FRAME_WIDTH
        || height > MAX_FRAME_HEIGHT
        || target_hz == 0
        || target_hz > UI4_VISUAL_SOFT_CAP_HZ
    {
        return 0;
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let payload = target_hz.to_le_bytes();
        let (status, window) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_UI4_SCENE_FRAME_OPEN_VISUAL,
            pack_i32_pair(x, y),
            pack_u32_pair(width, height),
            &payload,
            &mut [],
        );
        return if status == trueos_vm::vmcall::STATUS_OK {
            window as u32
        } else {
            0
        };
    }
    open_blueprint_frame(x, y, width, height, FrameCadence::Streaming, Some(target_hz))
}

fn open_blueprint_frame(
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    cadence: FrameCadence,
    visual_target_hz: Option<u32>,
) -> u32 {
    reap_retired_frames();
    let Some(owner) = blueprint_owner() else {
        return 0;
    };

    let output = OutputId::from_slot(0).expect("UI4 D01 must exist");
    // Visual compute is a synchronous full-frame producer. It needs one live
    // front and one producer back, not the generic streaming scene's third
    // queued allocation.
    let frame_cadence = if visual_target_hz.is_some() {
        FrameCadence::Dirty
    } else {
        cadence
    };
    let frame_spec = FrameSpec {
        output,
        content: FrameContent::BlueprintScene,
        cadence: frame_cadence,
        buffering: match frame_cadence {
            FrameCadence::Immutable => super::FrameBuffering::Single,
            FrameCadence::Dirty => super::FrameBuffering::Double,
            FrameCadence::Streaming => super::FrameBuffering::Triple,
        },
        format: ScanoutFormat::Rgba8888Premultiplied,
        width,
        height,
        base_color: visual_target_hz
            .is_none()
            .then_some(PremultipliedRgba8::TRANSPARENT),
    };
    let frame = match if visual_target_hz.is_some() {
        create_gpu_full_overwrite_frame(frame_spec)
    } else {
        create_frame(frame_spec)
    } {
        Ok(frame) => frame,
        Err(error) => {
            log_blueprint_frame_open_failure("frame-allocation", owner, width, height, error);
            return 0;
        }
    };
    let session = match begin_additional_window_session(owner) {
        Ok(session) => session,
        Err(error) => {
            let _ = destroy_frame(frame);
            log_blueprint_frame_open_failure("session-allocation", owner, width, height, error);
            return 0;
        }
    };
    let desktop_shell_launch = super::input_broker::claim_desktop_shell_launch(owner);
    let (x, y) = desktop_shell_launch.map_or((x, y), |launch| {
        let (screen_width, screen_height) =
            crate::intel::active_scanout_dimensions().unwrap_or((width, height));
        (
            launch.x.min(screen_width.saturating_sub(width)) as i32,
            launch.y.min(screen_height.saturating_sub(height)) as i32,
        )
    });
    let placement = WindowPlacement {
        x,
        y,
        width,
        height,
        z: 40,
        opacity: u8::MAX,
        visible: true,
    };
    let plane_slot = if cadence == FrameCadence::Streaming || visual_target_hz.is_some() {
        super::ALPHA_OVERLAY_PLANE_SLOT
    } else {
        super::RGB_OVERLAY_PLANE_SLOT_2
    };
    let window = match create_window(WindowCreate {
        owner,
        session,
        frame,
        output,
        plane: WindowPlane::Universal(plane_slot as u8),
        placement,
        interaction: super::WindowInteraction::APPLICATION,
    }) {
        Ok(window) => window,
        Err(error) => {
            let _ = finish_window_session(owner, session);
            let _ = destroy_frame(frame);
            log_blueprint_frame_open_failure("window-allocation", owner, width, height, error);
            return 0;
        }
    };

    let mut surfaces = SURFACES.lock();
    if surfaces.len() >= MAX_SURFACES {
        drop(surfaces);
        log_blueprint_frame_open_failure(
            "blueprint-surface-capacity",
            owner,
            width,
            height,
            MAX_SURFACES,
        );
        release_surface(
            BlueprintSceneSurface {
                owner,
                session,
                frame,
                window,
                width,
                height,
                cadence: frame_cadence,
                visual_cadence: visual_target_hz.map(BlueprintVisualCadence::new),
                pending_resize: None,
                write_lease: None,
                pending_gpu_release: None,
                pending_render_release: None,
                gpu_submission_unretired: false,
                vgpu_surface: None,
                particle_craft: None,
                placement,
                launch_selection: desktop_shell_launch
                    .map(|launch| (launch.source, launch.x, launch.y)),
                skybox: None,
                skybox_upload: None,
                sprites: Vec::new(),
                sprite_upload: None,
                solid_source: None,
                sprite_clear_rgba: None,
                sprite_scene_upload: None,
                pending_pointer_events: VecDeque::new(),
                pending_pan_events: VecDeque::new(),
                pending_resize_events: VecDeque::new(),
                pending_keyboard_events: VecDeque::new(),
                pending_keyboard_burst: VecDeque::new(),
                retained_text_layers: Vec::new(),
                retained_text_cursor: 0,
                retained_text_rendered: false,
                retained_text_backbuffer_extent: None,
                retained_text_backbuffer: None,
                font_canvas: None,
                stamped_text_layers: Vec::new(),
                stamped_text_cursor: 0,
                stamped_text_pending: None,
                stamped_text_rendered: false,
            },
            BlueprintSurfaceRelease::Animated,
        );
        return 0;
    }
    surfaces.push(BlueprintSceneSurface {
        owner,
        session,
        frame,
        window,
        width,
        height,
        cadence: frame_cadence,
        visual_cadence: visual_target_hz.map(BlueprintVisualCadence::new),
        pending_resize: None,
        write_lease: None,
        pending_gpu_release: None,
        pending_render_release: None,
        gpu_submission_unretired: false,
        vgpu_surface: None,
        particle_craft: None,
        placement,
        launch_selection: desktop_shell_launch.map(|launch| (launch.source, launch.x, launch.y)),
        skybox: None,
        skybox_upload: None,
        sprites: Vec::new(),
        sprite_upload: None,
        solid_source: None,
        sprite_clear_rgba: None,
        sprite_scene_upload: None,
        pending_pointer_events: VecDeque::new(),
        pending_pan_events: VecDeque::new(),
        pending_resize_events: VecDeque::new(),
        pending_keyboard_events: VecDeque::new(),
        pending_keyboard_burst: VecDeque::new(),
        retained_text_layers: Vec::new(),
        retained_text_cursor: 0,
        retained_text_rendered: false,
        retained_text_backbuffer_extent: None,
        retained_text_backbuffer: None,
        font_canvas: None,
        stamped_text_layers: Vec::new(),
        stamped_text_cursor: 0,
        stamped_text_pending: None,
        stamped_text_rendered: false,
    });
    let (cadence_name, buffer_count) = match frame_cadence {
        FrameCadence::Dirty => ("dirty", 2),
        FrameCadence::Streaming => ("streaming", 3),
        FrameCadence::Immutable => ("immutable", 1),
    };
    crate::log_info!(target: "ui4/blueprint-frame"; "frame open owner={:?} window={} extent={}x{} cadence={} visual_hz={} visual_soft_cap_hz={} buffers={} plane=slot{} scene=text+shader\n", owner, window.raw(), width, height, cadence_name, visual_target_hz.unwrap_or(0), UI4_VISUAL_SOFT_CAP_HZ, buffer_count, plane_slot);
    window.raw()
}

fn log_blueprint_frame_open_failure(
    stage: &str,
    owner: WindowOwner,
    width: u32,
    height: u32,
    error: impl core::fmt::Debug,
) {
    let usage = super::ui4_live_resource_usage();
    let pmm = crate::phys::pmm_stats();
    crate::log_error!(target: "ui4/blueprint-frame";
        "frame open rejected stage={} owner={:?} extent={}x{} error={:?} active_frames={} active_sessions={} live_windows={} pmm_free_bytes={} pmm_largest_free_bytes={} pmm_free_regions={} action=inspect-exact-admission-before-changing-frame-policy\n",
        stage,
        owner,
        width,
        height,
        error,
        usage.active_frames,
        usage.active_sessions,
        usage.live_windows,
        pmm.map_or(0, |stats| stats.free_bytes),
        pmm.map_or(0, |stats| stats.largest_free_region),
        pmm.map_or(0, |stats| stats.free_regions),
    );
}

/// Acquire and clear the non-front UI4 buffer for a new scene paint pass.
pub extern "C" fn trueos_cabi_ui4_solara_frame_begin(window_id: u32, clear_rgba: u32) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return guest_status(
            trueos_vm::vmcall::OP_BP_UI4_SOLARA_FRAME_BEGIN,
            window_id as u64,
            clear_rgba as u64,
            &[],
        );
    }
    reap_retired_frames();
    let Some(owner) = blueprint_owner() else {
        return ERROR_CONTEXT;
    };
    begin_blueprint_frame(owner, window_id, clear_rgba, true)
}

/// Acquire a UI4 write lease whose full clear is emitted by the first sprite
/// worklist submission rather than painted and flushed by the calling CPU.
pub extern "C" fn trueos_cabi_ui4_scene_sprite_frame_begin(window_id: u32, clear_rgba: u32) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return guest_status(
            trueos_vm::vmcall::OP_BP_UI4_SCENE_SPRITE_FRAME_BEGIN,
            window_id as u64,
            clear_rgba as u64,
            &[],
        );
    }
    reap_retired_frames();
    let Some(owner) = blueprint_owner() else {
        return ERROR_CONTEXT;
    };
    begin_blueprint_frame(owner, window_id, clear_rgba, false)
}

pub(crate) fn begin_vgpu_surface_import(
    owner: WindowOwner,
    window_id: u32,
) -> Result<crate::gpu::vgpu::Ui4SurfaceDescriptor, i32> {
    let mut surfaces = SURFACES.lock();
    let surface = surface_mut(&mut surfaces, owner, window_id).ok_or(ERROR_NOT_FOUND)?;
    if surface.vgpu_surface.is_some() || surface.gpu_submission_unretired {
        return Err(ERROR_BUSY);
    }
    let lease = surface.write_lease.ok_or(ERROR_STATE)?;
    let destination = gpgpu_rgba_surface(lease).map_err(|_| ERROR_UI4)?;
    surface.vgpu_surface = Some(0);
    surface.gpu_submission_unretired = true;
    Ok(crate::gpu::vgpu::Ui4SurfaceDescriptor {
        window_id,
        phys: destination.phys,
        producer_gpu: destination.gpu,
        bytes: destination.bytes,
        width: destination.width,
        height: destination.height,
        pitch: destination.pitch_bytes,
    })
}

pub(crate) fn complete_vgpu_surface_submission(
    owner: WindowOwner,
    window_id: u32,
    surface_handle: u64,
    release: GpgpuRgba8ReleaseFence,
) -> Result<(), i32> {
    let mut surfaces = SURFACES.lock();
    let surface = surface_mut(&mut surfaces, owner, window_id).ok_or(ERROR_NOT_FOUND)?;
    if surface.vgpu_surface != Some(surface_handle) || !surface.gpu_submission_unretired {
        return Err(ERROR_STATE);
    }
    let lease = surface.write_lease.ok_or(ERROR_STATE)?;
    let destination = gpgpu_rgba_surface(lease).map_err(|_| ERROR_UI4)?;
    if !release.matches(destination.phys, destination.bytes) {
        return Err(ERROR_UI4);
    }
    surface.vgpu_surface = None;
    surface.gpu_submission_unretired = false;
    surface.pending_gpu_release = Some(release);
    Ok(())
}

pub(crate) fn complete_vgpu_resident_surface_submission(
    owner: WindowOwner,
    window_id: u32,
    surface_handle: u64,
    release: crate::intel::render::ResidentSceneReleaseFence,
) -> Result<(), i32> {
    let mut surfaces = SURFACES.lock();
    let surface = surface_mut(&mut surfaces, owner, window_id).ok_or(ERROR_NOT_FOUND)?;
    if surface.vgpu_surface != Some(surface_handle) || !surface.gpu_submission_unretired {
        return Err(ERROR_STATE);
    }
    let lease = surface.write_lease.ok_or(ERROR_STATE)?;
    let destination = gpgpu_rgba_surface(lease).map_err(|_| ERROR_UI4)?;
    if !release.matches(destination.phys, destination.bytes) {
        return Err(ERROR_UI4);
    }
    surface.vgpu_surface = None;
    surface.gpu_submission_unretired = false;
    surface.pending_render_release = Some(release);
    Ok(())
}

pub(crate) fn commit_vgpu_surface_import(
    owner: WindowOwner,
    window_id: u32,
    surface_handle: u64,
) -> Result<(), i32> {
    if surface_handle == 0 {
        return Err(ERROR_INVALID);
    }
    let mut surfaces = SURFACES.lock();
    let surface = surface_mut(&mut surfaces, owner, window_id).ok_or(ERROR_NOT_FOUND)?;
    if surface.vgpu_surface != Some(0) || !surface.gpu_submission_unretired {
        return Err(ERROR_STATE);
    }
    surface.vgpu_surface = Some(surface_handle);
    Ok(())
}

pub(crate) fn abort_vgpu_surface_import(owner: WindowOwner, window_id: u32) {
    let mut surfaces = SURFACES.lock();
    if let Some(surface) = surface_mut(&mut surfaces, owner, window_id)
        && surface.vgpu_surface == Some(0)
    {
        surface.vgpu_surface = None;
        surface.gpu_submission_unretired = false;
    }
}

pub(crate) fn complete_vgpu_surface_discard(
    owner: WindowOwner,
    window_id: u32,
    surface_handle: u64,
) -> Result<(), i32> {
    let mut surfaces = SURFACES.lock();
    let surface = surface_mut(&mut surfaces, owner, window_id).ok_or(ERROR_NOT_FOUND)?;
    if surface.vgpu_surface != Some(surface_handle) || !surface.gpu_submission_unretired {
        return Err(ERROR_STATE);
    }
    let lease = surface.write_lease.ok_or(ERROR_STATE)?;
    cancel_frame_buffer(lease).map_err(|_| ERROR_UI4)?;
    surface.vgpu_surface = None;
    surface.gpu_submission_unretired = false;
    surface.pending_gpu_release = None;
    surface.pending_render_release = None;
    surface.write_lease = None;
    Ok(())
}

/// Wait in the kernel for one visual cadence ticket, then acquire its exact
/// GPU-only back buffer. VM dispatch keeps this request pending across the
/// deadline; callers do not need a millisecond retry loop.
pub extern "C" fn trueos_cabi_ui4_scene_visual_frame_begin(window_id: u32) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return guest_status(
            trueos_vm::vmcall::OP_BP_UI4_SCENE_VISUAL_FRAME_BEGIN,
            window_id as u64,
            0,
            &[],
        );
    }
    reap_retired_frames();
    let Some(owner) = blueprint_owner() else {
        return ERROR_CONTEXT;
    };
    if visual_frame_retry_ms(owner, window_id).is_none() {
        return ERROR_STATE;
    }
    begin_blueprint_frame(owner, window_id, 0, false)
}

/// Return the exact remaining visual deadline, or one scheduler tick when the
/// cadence is ready but frame/display ownership still applies backpressure.
pub(crate) fn visual_frame_retry_ms(owner: WindowOwner, window_id: u32) -> Option<u64> {
    let mut surfaces = SURFACES.lock();
    let surface = surface_mut(&mut surfaces, owner, window_id)?;
    surface
        .visual_cadence
        .as_ref()
        .map(|cadence| cadence.wait_ms().max(1))
}

pub(crate) fn begin_blueprint_frame(
    owner: WindowOwner,
    window_id: u32,
    clear_rgba: u32,
    cpu_clear: bool,
) -> i32 {
    let mut surfaces = SURFACES.lock();
    let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
        return ERROR_NOT_FOUND;
    };
    if surface.write_lease.is_some() {
        return ERROR_STATE;
    }
    if surface.gpu_submission_unretired {
        return ERROR_BUSY;
    }
    if let Some(cadence) = surface.visual_cadence.as_ref()
        && cadence.wait_ms() != 0
    {
        return ERROR_BUSY;
    }
    let lease = match acquire_frame_buffer(surface.frame) {
        Ok(lease) => lease,
        Err(FramePoolError::ImmutablePublished) if surface.cadence == FrameCadence::Immutable => {
            let output = OutputId::from_slot(0).expect("UI4 D01 must exist");
            let replacement = match create_frame(FrameSpec {
                output,
                content: FrameContent::BlueprintScene,
                cadence: FrameCadence::Immutable,
                buffering: super::FrameBuffering::Single,
                format: ScanoutFormat::Rgba8888Premultiplied,
                width: surface.width,
                height: surface.height,
                base_color: Some(PremultipliedRgba8::TRANSPARENT),
            }) {
                Ok(frame) => frame,
                Err(error) => {
                    crate::log_warn!(target: "ui4/blueprint-frame"; "immutable refresh allocation failed owner={:?} window={} old_frame={} error={:?} action=retain-surflive-front\n", owner, window_id, surface.frame.raw(), error);
                    return ERROR_UI4;
                }
            };
            match acquire_frame_buffer(replacement) {
                Ok(lease) => {
                    crate::log_info!(target: "ui4/blueprint-frame"; "immutable refresh prepared owner={:?} window={} old_frame={} replacement_frame={} extent={}x{} action=paint-before-broker-swap\n", owner, window_id, surface.frame.raw(), replacement.raw(), surface.width, surface.height);
                    lease
                }
                Err(error) => {
                    let _ = destroy_frame(replacement);
                    crate::log_warn!(target: "ui4/blueprint-frame"; "immutable refresh acquire failed owner={:?} window={} replacement_frame={} error={:?} action=retain-surflive-front\n", owner, window_id, replacement.raw(), error);
                    return ERROR_UI4;
                }
            }
        }
        Err(FramePoolError::Busy) => return ERROR_BUSY,
        Err(error) => {
            crate::log_warn!(target: "ui4/blueprint-frame"; "frame begin failed owner={:?} window={} frame={} error={:?}\n", owner, window_id, surface.frame.raw(), error);
            return ERROR_UI4;
        }
    };
    let view = match writable_rgba_view(lease) {
        Ok(view) => view,
        Err(_) => {
            let _ = cancel_frame_buffer(lease);
            if lease.frame != surface.frame {
                let _ = destroy_frame(lease.frame);
            }
            return ERROR_UI4;
        }
    };
    // A retained streaming skybox is required to shade the complete target,
    // so clearing and flushing every CPU-visible cache line first is redundant.
    // Dirty/text frames retain the ordinary clear contract.
    let gpu_full_frame =
        !cpu_clear || (surface.cadence == FrameCadence::Streaming && surface.skybox.is_some());
    if !gpu_full_frame {
        let [r, g, b, a] = clear_rgba.to_le_bytes();
        let pixel = PremultipliedRgba8::from_straight_rgba(r, g, b, a).to_native_bytes();
        // SAFETY: the write lease makes this entire UI4 allocation producer-owned.
        let bytes = unsafe { core::slice::from_raw_parts_mut(view.virt, view.byte_len) };
        for chunk in bytes.chunks_exact_mut(4) {
            chunk.copy_from_slice(&pixel);
        }
        crate::intel::dma_flush(view.virt, view.byte_len);
        if a == u8::MAX {
            let _ = mark_frame_buffer_fully_opaque(lease);
        }
    }
    surface.pending_gpu_release = None;
    surface.pending_render_release = None;
    surface.sprite_clear_rgba = (!cpu_clear).then_some(clear_rgba);
    surface.sprite_scene_upload = None;
    surface.retained_text_cursor = 0;
    surface.retained_text_rendered = false;
    surface.stamped_text_cursor = 0;
    surface.stamped_text_pending = None;
    surface.stamped_text_rendered = false;
    surface.write_lease = Some(lease);
    if let Some(cadence) = surface.visual_cadence.as_mut() {
        cadence.consume_admission();
    }
    0
}

/// Take one pointer event scoped to the globally selected Blueprint frame.
/// A return value of one means the queue is currently empty; zero writes one
/// event. The absorb-select gesture never enters this queue.
pub unsafe extern "C" fn trueos_cabi_ui4_scene_pointer_event_take(
    window_id: u32,
    out: *mut TrueosUi4PointerEvent,
) -> i32 {
    if out.is_null() {
        return ERROR_INVALID;
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return unsafe { guest_pointer_event_take(window_id, out) };
    }
    let Some(owner) = blueprint_owner() else {
        return ERROR_CONTEXT;
    };
    {
        let mut surfaces = SURFACES.lock();
        if surface_mut(&mut surfaces, owner, window_id).is_none() {
            return ERROR_NOT_FOUND;
        }
    }

    route_owner_input_events(owner);
    let mut surfaces = SURFACES.lock();
    let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
        return ERROR_NOT_FOUND;
    };
    let Some(event) = surface.pending_pointer_events.pop_front() else {
        return 1;
    };
    // SAFETY: the non-null output points to one writable ABI event.
    unsafe { out.write(event) };
    0
}

/// Bounded landing area for context-menu outcomes. UI4's callback is a plain
/// kernel function pointer with no captured state, so the result is parked here
/// against its own window until the Blueprint takes it.
static CONTEXT_MENU_EVENTS: Mutex<VecDeque<(WindowOwner, WindowId, TrueosUi4ContextMenuEvent)>> =
    Mutex::new(VecDeque::new());
const MAX_PENDING_CONTEXT_MENU_EVENTS: usize = 16;

/// UI4's completion callback for every Blueprint-requested menu.
fn blueprint_context_menu_complete(result: crate::ui4::ContextMenuResult) {
    let reason = match result.reason {
        crate::ui4::ContextMenuCloseReason::Selected => 0,
        crate::ui4::ContextMenuCloseReason::Dismissed => 1,
        crate::ui4::ContextMenuCloseReason::Replaced => 2,
        crate::ui4::ContextMenuCloseReason::OwnerReleased => 3,
        crate::ui4::ContextMenuCloseReason::WindowClosed => 4,
    };
    let event = TrueosUi4ContextMenuEvent {
        context: result.context,
        action_id: result.selected_action.unwrap_or(0),
        selected: u32::from(result.selected_action.is_some()),
        reason,
    };
    let mut events = CONTEXT_MENU_EVENTS.lock();
    // One invocation is one outcome. A Blueprint which never polls must not be
    // able to grow kernel memory, so the oldest outcome yields.
    if events.len() >= MAX_PENDING_CONTEXT_MENU_EVENTS {
        events.pop_front();
    }
    events.push_back((result.owner, result.window, event));
}

/// Give this frame a standing context menu, replacing any previous one.
///
/// The frame owns the menu over its own pixels: UI4 raises it when a secondary
/// click lands on this window, and leaves that gesture to the kernel's desktop
/// menu for windows which register nothing. Passing zero entries clears the
/// registration. Outcomes arrive through
/// [`trueos_cabi_ui4_context_menu_event_take`].
pub unsafe extern "C" fn trueos_cabi_ui4_context_menu_register(
    window_id: u32,
    entries: *const TrueosUi4ContextMenuEntry,
    entry_count: usize,
) -> i32 {
    if entry_count > crate::ui4::MAX_CONTEXT_MENU_ENTRIES || (entry_count != 0 && entries.is_null())
    {
        return ERROR_INVALID;
    }
    // SAFETY: the C ABI requires `entry_count` readable entry records.
    let raw_entries = if entry_count == 0 {
        &[][..]
    } else {
        unsafe { core::slice::from_raw_parts(entries, entry_count) }
    };

    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return unsafe { guest_context_menu_register(window_id, raw_entries) };
    }

    let mut menu_entries = Vec::with_capacity(entry_count);
    for entry in raw_entries {
        if entry.label_ptr.is_null()
            || entry.label_len == 0
            || entry.label_len > MAX_CONTEXT_MENU_LABEL_BYTES
        {
            return ERROR_INVALID;
        }
        // SAFETY: the C ABI requires `label_len` readable bytes per entry.
        let label = unsafe { core::slice::from_raw_parts(entry.label_ptr, entry.label_len) };
        let Ok(label) = core::str::from_utf8(label) else {
            return ERROR_INVALID;
        };
        menu_entries.push(if entry.enabled == 0 {
            crate::ui4::ContextMenuEntry::disabled(label)
        } else {
            crate::ui4::ContextMenuEntry::action(label, entry.action_id)
        });
    }

    let Some(owner) = blueprint_owner() else {
        return ERROR_CONTEXT;
    };
    let window = {
        let mut surfaces = SURFACES.lock();
        let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
            return ERROR_NOT_FOUND;
        };
        surface.window
    };
    if menu_entries.is_empty() {
        crate::ui4::clear_window_context_menu(owner, window);
        return 0;
    }
    match crate::ui4::register_window_context_menu(
        owner,
        window,
        u64::from(window_id),
        menu_entries,
        blueprint_context_menu_complete,
    ) {
        Ok(()) => 0,
        Err(_) => ERROR_INVALID,
    }
}

/// Take one context-menu outcome for this window. A return value of one means
/// no invocation has completed; zero writes one outcome.
pub unsafe extern "C" fn trueos_cabi_ui4_context_menu_event_take(
    window_id: u32,
    out: *mut TrueosUi4ContextMenuEvent,
) -> i32 {
    if out.is_null() {
        return ERROR_INVALID;
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return unsafe { guest_context_menu_event_take(window_id, out) };
    }
    let Some(owner) = blueprint_owner() else {
        return ERROR_CONTEXT;
    };
    let window = {
        let mut surfaces = SURFACES.lock();
        let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
            return ERROR_NOT_FOUND;
        };
        surface.window
    };
    let mut events = CONTEXT_MENU_EVENTS.lock();
    let Some(index) = events.iter().position(|(queued_owner, queued_window, _)| {
        *queued_owner == owner && *queued_window == window
    }) else {
        return 1;
    };
    let (_, _, event) = events.remove(index).expect("located context menu outcome");
    drop(events);
    // SAFETY: the non-null output points to one writable ABI event.
    unsafe { out.write(event) };
    0
}

/// Take one keyboard event after UI4 has routed it to this exact Blueprint
/// window. A return value of one means the queue is currently empty; zero
/// writes one event. Text bursts are staged until their END marker arrives, so
/// an incomplete paste is never exposed as a complete window input operation.
pub unsafe extern "C" fn trueos_cabi_ui4_scene_keyboard_event_take(
    window_id: u32,
    out: *mut crate::r::keyboard::TrueosKeyboardOutputEvent,
) -> i32 {
    if out.is_null() {
        return ERROR_INVALID;
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return unsafe { guest_keyboard_event_take(window_id, out) };
    }
    let Some(owner) = blueprint_owner() else {
        return ERROR_CONTEXT;
    };
    {
        let mut surfaces = SURFACES.lock();
        if surface_mut(&mut surfaces, owner, window_id).is_none() {
            return ERROR_NOT_FOUND;
        }
    }

    route_owner_input_events(owner);
    let mut surfaces = SURFACES.lock();
    let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
        return ERROR_NOT_FOUND;
    };
    let Some(event) = surface.pending_keyboard_events.pop_front() else {
        return 1;
    };
    // SAFETY: the non-null output points to one writable ABI event.
    unsafe { out.write(event) };
    0
}

/// Take one middle-button pan gesture event scoped to this Blueprint window.
///
/// UI4 owns hit testing and pointer capture. The Blueprint owns the resulting
/// scene offset and repaint policy. A return value of one means the queue is
/// currently empty; zero writes one event.
pub unsafe extern "C" fn trueos_cabi_ui4_scene_pan_event_take(
    window_id: u32,
    out: *mut TrueosUi4PanEvent,
) -> i32 {
    if out.is_null() {
        return ERROR_INVALID;
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return unsafe { guest_pan_event_take(window_id, out) };
    }
    let Some(owner) = blueprint_owner() else {
        return ERROR_CONTEXT;
    };
    {
        let mut surfaces = SURFACES.lock();
        if surface_mut(&mut surfaces, owner, window_id).is_none() {
            return ERROR_NOT_FOUND;
        }
    }

    route_owner_input_events(owner);
    let mut surfaces = SURFACES.lock();
    let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
        return ERROR_NOT_FOUND;
    };
    let Some(event) = surface.pending_pan_events.pop_front() else {
        return 1;
    };
    // SAFETY: the non-null output points to one writable ABI event.
    unsafe { out.write(event) };
    0
}

/// Take this window's first physical-presentation event.
///
/// Zero means the compositor observed the first frame at SURFLIVE and this
/// call consumed the event. One means it has not arrived yet or was already
/// consumed. Negative values are ordinary UI4 errors.
pub extern "C" fn trueos_cabi_ui4_scene_first_presentation_take(window_id: u32) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return guest_status(
            trueos_vm::vmcall::OP_BP_UI4_SCENE_FIRST_PRESENTATION_TAKE,
            window_id as u64,
            0,
            &[],
        );
    }
    let Some(owner) = blueprint_owner() else {
        return ERROR_CONTEXT;
    };
    let window = {
        let mut surfaces = SURFACES.lock();
        let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
            return ERROR_NOT_FOUND;
        };
        surface.window
    };
    match take_window_first_presentation(owner, window) {
        Ok(true) => 0,
        Ok(false) => 1,
        Err(_) => ERROR_UI4,
    }
}

/// Return the cursor/UI4 output extent packed as `width << 32 | height`.
///
/// Zero means that no usable output geometry is available.
pub extern "C" fn trueos_cabi_ui4_scene_output_dimensions() -> u64 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, dimensions) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_UI4_SCENE_OUTPUT_DIMENSIONS,
            0,
            0,
            &[],
            &mut [],
        );
        return if status == trueos_vm::vmcall::STATUS_OK {
            dimensions
        } else {
            0
        };
    }
    let (width, height) = crate::r::io::input_cabi::input_cursor_viewport_dimensions_px();
    let Ok(width) = u32::try_from(width) else {
        return 0;
    };
    let Ok(height) = u32::try_from(height) else {
        return 0;
    };
    if width == 0 || height == 0 {
        return 0;
    }
    pack_u32_pair(width, height)
}

/// Take the next maximize/restore extent notification for this Blueprint
/// window. A return value of one means the queue is currently empty; zero
/// writes one event. The producer chooses whether and when to resize.
pub unsafe extern "C" fn trueos_cabi_ui4_scene_resize_event_take(
    window_id: u32,
    out: *mut TrueosUi4ResizeEvent,
) -> i32 {
    if out.is_null() {
        return ERROR_INVALID;
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return unsafe { guest_resize_event_take(window_id, out) };
    }
    let Some(owner) = blueprint_owner() else {
        return ERROR_CONTEXT;
    };
    {
        let mut surfaces = SURFACES.lock();
        if surface_mut(&mut surfaces, owner, window_id).is_none() {
            return ERROR_NOT_FOUND;
        }
    }

    route_owner_input_events(owner);
    let mut surfaces = SURFACES.lock();
    let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
        return ERROR_NOT_FOUND;
    };
    let Some(event) = surface.pending_resize_events.pop_front() else {
        return 1;
    };
    // SAFETY: the non-null output points to one writable ABI event.
    unsafe { out.write(event) };
    0
}

/// Read the held HID usages for the keyboard routed to this selected window.
///
/// This is window-scoped state, not the global HUT inventory. A return value
/// of one means the window currently has no keyboard-bearing focus route.
pub unsafe extern "C" fn trueos_cabi_ui4_scene_keyboard_state(
    window_id: u32,
    out: *mut TrueosUi4KeyboardState,
) -> i32 {
    if out.is_null() {
        return ERROR_INVALID;
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return unsafe { guest_keyboard_state(window_id, out) };
    }
    let Some(owner) = blueprint_owner() else {
        return ERROR_CONTEXT;
    };
    let window = {
        let mut surfaces = SURFACES.lock();
        let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
            return ERROR_NOT_FOUND;
        };
        surface.window
    };
    route_owner_input_events(owner);
    let Some(keyboard) = focused_keyboard_state(owner, window) else {
        return 1;
    };
    let state = TrueosUi4KeyboardState {
        controller_id: keyboard.controller_id,
        slot_id: keyboard.slot_id,
        ep_target: keyboard.ep_target,
        combo_id: keyboard.combo_id,
        modifiers: keyboard.modifiers,
        source_kind: keyboard.source_kind as u8,
        virtual_keyboard: u8::from(keyboard.virtual_device),
        reserved0: 0,
        keys: keyboard.keys,
        ascii: keyboard.ascii,
        key_down_bits: keyboard.key_down_bits,
    };
    // SAFETY: the non-null output points to one writable ABI state record.
    unsafe { out.write(state) };
    0
}

/// Read all UI4 cursor/combo routes and their paired held-key state.
///
/// The returned count is the total route count. At most `out_cap` records are
/// written. Records remain visible when their cursor selects another frame so
/// a joined local player can be reported as missing focus without exposing
/// another window's identity.
pub unsafe extern "C" fn trueos_cabi_ui4_scene_input_routes(
    window_id: u32,
    out: *mut TrueosUi4InputRouteState,
    out_cap: u32,
) -> isize {
    if out_cap != 0 && out.is_null() {
        return ERROR_INVALID as isize;
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return unsafe { guest_input_routes(window_id, out, out_cap) };
    }
    let Some(owner) = blueprint_owner() else {
        return ERROR_CONTEXT as isize;
    };
    let window = {
        let mut surfaces = SURFACES.lock();
        let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
            return ERROR_NOT_FOUND as isize;
        };
        surface.window
    };
    route_owner_input_events(owner);
    let routes = window_input_routes(owner, window);
    for (index, route) in routes.iter().take(out_cap as usize).enumerate() {
        let mut flags = 0;
        if route.selected_for_window {
            flags |= UI4_INPUT_ROUTE_SELECTED_FOR_WINDOW;
        }
        if route.app_focus {
            flags |= UI4_INPUT_ROUTE_APP_FOCUS;
        }
        if route.vcursor {
            flags |= UI4_INPUT_ROUTE_VCURSOR;
        }
        let mut state = TrueosUi4InputRouteState {
            cursor_controller_id: route.source.controller_id,
            cursor_slot_id: route.source.slot_id,
            cursor_ep_target: route.source.ep_target,
            cursor_hid_kind: u32::from(route.source.hid_kind),
            combo_id: route.combo_id,
            color_rgba: u32::from_le_bytes([
                route.color.r,
                route.color.g,
                route.color.b,
                route.color.a,
            ]),
            flags,
            ..TrueosUi4InputRouteState::default()
        };
        if let Some(keyboard) = route.keyboard.as_ref() {
            state.flags |= UI4_INPUT_ROUTE_KEYBOARD_PRESENT;
            state.keyboard_controller_id = keyboard.controller_id;
            state.keyboard_slot_id = keyboard.slot_id;
            state.keyboard_ep_target = keyboard.ep_target;
            state.keyboard_modifiers = keyboard.modifiers;
            state.keyboard_source_kind = keyboard.source_kind as u8;
            state.virtual_keyboard = u8::from(keyboard.virtual_device);
            state.keys = keyboard.keys;
            state.ascii = keyboard.ascii;
            state.key_down_bits = keyboard.key_down_bits;
        }
        // SAFETY: the caller promised out_cap writable records.
        unsafe { out.add(index).write(state) };
    }
    routes.len() as isize
}

/// Drain one VM owner's broker queue once and preserve every pointer, pan,
/// resize, and keyboard event for its exact surface. Keyboard held state
/// remains available separately through the input broker snapshot contract.
fn route_owner_input_events(owner: WindowOwner) {
    let input_events = take_owner_input_events(owner);
    if input_events.is_empty() {
        return;
    }
    let mut surfaces = SURFACES.lock();
    for input in input_events {
        match input {
            Ui4InputEvent::Pointer(event) => {
                let Some(surface) = surface_mut(&mut surfaces, owner, event.window.raw()) else {
                    continue;
                };
                if surface.pending_pointer_events.len() >= MAX_PENDING_POINTER_EVENTS {
                    continue;
                }
                surface
                    .pending_pointer_events
                    .push_back(TrueosUi4PointerEvent {
                        controller_id: event.source.controller_id,
                        slot_id: event.source.slot_id,
                        ep_target: event.source.ep_target,
                        hid_kind: u32::from(event.source.hid_kind),
                        x: event.x,
                        y: event.y,
                        local_x: event.local_x,
                        local_y: event.local_y,
                        dx: event.dx,
                        dy: event.dy,
                        wheel: i32::from(event.wheel),
                        buttons_down: event.buttons_down,
                        buttons_pressed: event.buttons_pressed,
                        buttons_released: event.buttons_released,
                        combo_id: event.combo_id,
                        vcursor: u32::from(event.vcursor),
                    });
            }
            Ui4InputEvent::Pan(event) => {
                let Some(surface) = surface_mut(&mut surfaces, owner, event.window.raw()) else {
                    continue;
                };
                if surface.pending_pan_events.len() >= MAX_PENDING_PAN_EVENTS {
                    continue;
                }
                let phase = match event.phase {
                    super::Ui4PanPhase::Begin => 1,
                    super::Ui4PanPhase::Update => 2,
                    super::Ui4PanPhase::End => 3,
                };
                surface.pending_pan_events.push_back(TrueosUi4PanEvent {
                    controller_id: event.source.controller_id,
                    slot_id: event.source.slot_id,
                    ep_target: event.source.ep_target,
                    hid_kind: u32::from(event.source.hid_kind),
                    phase,
                    x: event.x,
                    y: event.y,
                    local_x: event.local_x,
                    local_y: event.local_y,
                    dx: event.dx,
                    dy: event.dy,
                    combo_id: event.combo_id,
                    vcursor: u32::from(event.vcursor),
                });
            }
            Ui4InputEvent::Resize(event) => {
                let Some(surface) = surface_mut(&mut surfaces, owner, event.window.raw()) else {
                    continue;
                };
                // Extents are target state, not deltas. Only the newest
                // maximize/restore request matters if the producer has not
                // serviced an older one yet.
                surface.pending_resize_events.clear();
                surface
                    .pending_resize_events
                    .push_back(TrueosUi4ResizeEvent {
                        old_width: event.old_width,
                        old_height: event.old_height,
                        width: event.width,
                        height: event.height,
                    });
            }
            Ui4InputEvent::Keyboard(event) => {
                let Some(surface) = surface_mut(&mut surfaces, owner, event.window.raw()) else {
                    continue;
                };
                enqueue_window_keyboard_event(
                    &mut surface.pending_keyboard_events,
                    &mut surface.pending_keyboard_burst,
                    event.event,
                );
            }
            _ => {}
        }
    }
}

fn enqueue_window_keyboard_event(
    pending: &mut VecDeque<crate::r::keyboard::TrueosKeyboardOutputEvent>,
    burst: &mut VecDeque<crate::r::keyboard::TrueosKeyboardOutputEvent>,
    event: crate::r::keyboard::TrueosKeyboardOutputEvent,
) {
    let burst_member = event.flags & crate::r::keyboard::KEYBOARD_OUTPUT_FLAG_TEXT_BURST != 0;
    if !burst_member {
        // A non-burst transition after a truncated burst proves that END was
        // lost. Discard the unpublished prefix, then preserve the transition.
        burst.clear();
        if pending.len() < MAX_PENDING_KEYBOARD_EVENTS {
            pending.push_back(event);
        }
        return;
    }

    let burst_start = event.flags & crate::r::keyboard::KEYBOARD_OUTPUT_FLAG_TEXT_BURST_START != 0;
    let burst_end = event.flags & crate::r::keyboard::KEYBOARD_OUTPUT_FLAG_TEXT_BURST_END != 0;

    if burst_start {
        // A newer START also terminates any unpublished, incomplete prefix.
        burst.clear();
        if event.device_seq == 0
            || event.flags & crate::r::keyboard::KEYBOARD_OUTPUT_FLAG_SYNTHETIC == 0
        {
            return;
        }
    } else {
        let Some(previous) = burst.back().copied() else {
            // This is a tail whose START was lost upstream.
            return;
        };
        let same_burst = event.device_seq == previous.device_seq
            && event.controller_id == previous.controller_id
            && event.slot_id == previous.slot_id
            && event.ep_target == previous.ep_target
            && event.seq == previous.seq.wrapping_add(1);
        if !same_burst {
            burst.clear();
            return;
        }
    }

    if burst.len() >= MAX_PENDING_KEYBOARD_EVENTS {
        burst.clear();
        return;
    }
    burst.push_back(event);
    if !burst_end {
        return;
    }

    // Publish the burst to the window queue only when every contiguous scalar
    // including END fits. Otherwise discard the whole operation atomically.
    if pending.len().saturating_add(burst.len()) <= MAX_PENDING_KEYBOARD_EVENTS {
        pending.append(burst);
    } else {
        burst.clear();
    }
}

/// Move a Blueprint scene window without exposing a broker handle.
pub extern "C" fn trueos_cabi_ui4_scene_frame_set_position(window_id: u32, x: i32, y: i32) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return guest_status(
            trueos_vm::vmcall::OP_BP_UI4_SCENE_FRAME_SET_POSITION,
            window_id as u64,
            pack_i32_pair(x, y),
            &[],
        );
    }
    let Some(owner) = blueprint_owner() else {
        return ERROR_CONTEXT;
    };
    let mut surfaces = SURFACES.lock();
    let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
        return ERROR_NOT_FOUND;
    };
    let placement = WindowPlacement {
        x,
        y,
        ..surface.placement
    };
    if set_window_placement(owner, surface.window, placement).is_err() {
        return ERROR_UI4;
    }
    surface.placement = placement;
    0
}

/// Include or exclude this Blueprint frame from UI4 cursor hit testing.
pub extern "C" fn trueos_cabi_ui4_scene_frame_set_hit_testable(
    window_id: u32,
    enabled: u32,
) -> i32 {
    if enabled > 1 {
        return ERROR_INVALID;
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return guest_status(
            trueos_vm::vmcall::OP_BP_UI4_SCENE_FRAME_SET_HIT_TESTABLE,
            window_id as u64,
            enabled as u64,
            &[],
        );
    }
    let Some(owner) = blueprint_owner() else {
        return ERROR_CONTEXT;
    };
    let window = {
        let mut surfaces = SURFACES.lock();
        let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
            return ERROR_NOT_FOUND;
        };
        surface.window
    };
    if set_window_hit_testable(owner, window, enabled != 0).is_err() {
        return ERROR_UI4;
    }
    0
}

/// Compatibility shorthand for a frame-wide `AppOwned` cursor override.
/// Overrides are retained per frame but activate only for the one globally
/// selected frame while the cursor is inside that frame.
pub extern "C" fn trueos_cabi_ui4_scene_set_custom_cursor(window_id: u32, enabled: u32) -> i32 {
    if enabled > 1 {
        return ERROR_INVALID;
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return guest_status(
            trueos_vm::vmcall::OP_BP_UI4_SCENE_SET_CUSTOM_CURSOR,
            window_id as u64,
            enabled as u64,
            &[],
        );
    }
    let Some(owner) = blueprint_owner() else {
        return ERROR_CONTEXT;
    };
    let window = {
        let mut surfaces = SURFACES.lock();
        let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
            return ERROR_NOT_FOUND;
        };
        surface.window
    };
    if set_window_custom_cursor(owner, window, enabled != 0).is_err() {
        return ERROR_UI4;
    }
    0
}

/// Set a kernel cursor sprite for this frame. A null source changes the
/// frame-wide fallback; a source selects only that one cursor route.
pub unsafe extern "C" fn trueos_cabi_ui4_scene_set_cursor_icon(
    window_id: u32,
    source: *const TrueosUi4CursorSource,
    icon: u32,
) -> i32 {
    let Some(icon) = Ui4CursorIcon::from_raw(icon) else {
        return ERROR_INVALID;
    };
    let source = if source.is_null() {
        None
    } else {
        // SAFETY: the C ABI requires one readable source record.
        let source = unsafe { source.read() };
        let Ok(hid_kind) = u8::try_from(source.hid_kind) else {
            return ERROR_INVALID;
        };
        Some(Ui4CursorSource {
            controller_id: source.controller_id,
            slot_id: source.slot_id,
            ep_target: source.ep_target,
            hid_kind,
        })
    };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let payload = source.map(|source| TrueosUi4CursorSource {
            controller_id: source.controller_id,
            slot_id: source.slot_id,
            ep_target: source.ep_target,
            hid_kind: u32::from(source.hid_kind),
        });
        let payload = payload.as_ref().map_or(&[][..], |source| unsafe {
            core::slice::from_raw_parts(
                (source as *const TrueosUi4CursorSource).cast::<u8>(),
                core::mem::size_of::<TrueosUi4CursorSource>(),
            )
        });
        return guest_status(
            trueos_vm::vmcall::OP_BP_UI4_SCENE_SET_CURSOR_ICON,
            window_id as u64,
            icon as u64,
            payload,
        );
    }
    let Some(owner) = blueprint_owner() else {
        return ERROR_CONTEXT;
    };
    let window = {
        let mut surfaces = SURFACES.lock();
        let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
            return ERROR_NOT_FOUND;
        };
        surface.window
    };
    if set_window_cursor_icon(owner, window, source, icon).is_err() {
        return ERROR_UI4;
    }
    0
}

/// Replace the backing frame while retaining the UI4 window and scene assets.
pub extern "C" fn trueos_cabi_ui4_scene_frame_resize(
    window_id: u32,
    width: u32,
    height: u32,
) -> i32 {
    if width == 0 || height == 0 || width > MAX_FRAME_WIDTH || height > MAX_FRAME_HEIGHT {
        return ERROR_INVALID;
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return guest_status(
            trueos_vm::vmcall::OP_BP_UI4_SCENE_FRAME_RESIZE,
            window_id as u64,
            pack_u32_pair(width, height),
            &[],
        );
    }
    reap_retired_frames();
    let Some(owner) = blueprint_owner() else {
        return ERROR_CONTEXT;
    };
    let (cadence, window, particle_craft, visual, current_frame, previous_frame) = {
        let mut surfaces = SURFACES.lock();
        let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
            return ERROR_NOT_FOUND;
        };
        if surface.write_lease.is_some() {
            return ERROR_STATE;
        }
        (
            surface.cadence,
            surface.window,
            surface.particle_craft.is_some(),
            surface.visual_cadence.is_some(),
            surface.frame,
            surface
                .pending_resize
                .map_or(surface.frame, |pending| pending.previous_frame),
        )
    };
    let (backing_width, backing_height) = if particle_craft {
        crate::intel::gpgpu::particle_craft_backing_extent(width, height)
    } else {
        (width, height)
    };
    let output = OutputId::from_slot(0).expect("UI4 D01 must exist");
    let replacement_spec = FrameSpec {
        output,
        content: FrameContent::BlueprintScene,
        cadence,
        buffering: match cadence {
            FrameCadence::Immutable => super::FrameBuffering::Single,
            FrameCadence::Dirty => super::FrameBuffering::Double,
            FrameCadence::Streaming => super::FrameBuffering::Triple,
        },
        format: ScanoutFormat::Rgba8888Premultiplied,
        width: backing_width,
        height: backing_height,
        base_color: (!visual).then_some(PremultipliedRgba8::TRANSPARENT),
    };
    let replacement = match if visual {
        create_gpu_full_overwrite_frame(replacement_spec)
    } else {
        create_frame(replacement_spec)
    } {
        Ok(frame) => frame,
        Err(error) => {
            crate::log_error!(
                target: "ui4/blueprint-frame";
                "frame resize allocation failed owner={:?} window={} logical_extent={}x{} backing_extent={}x{} buffering={:?} error={:?}\n",
                owner,
                window_id,
                width,
                height,
                backing_width,
                backing_height,
                cadence,
                error,
            );
            return ERROR_UI4;
        }
    };

    let live_placement = match window_placement(owner, window) {
        Ok(placement) => placement,
        Err(error) => {
            let _ = destroy_frame(replacement);
            crate::log_error!(
                target: "ui4/blueprint-frame";
                "frame resize placement lookup failed owner={:?} window={} error={:?}\n",
                owner,
                window_id,
                error,
            );
            return ERROR_UI4;
        }
    };
    let superseded = {
        let mut surfaces = SURFACES.lock();
        let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
            let _ = destroy_frame(replacement);
            return ERROR_NOT_FOUND;
        };
        if surface.write_lease.is_some() || surface.frame != current_frame {
            let _ = destroy_frame(replacement);
            return ERROR_STATE;
        }
        let placement = WindowPlacement {
            width,
            height,
            ..live_placement
        };
        let superseded = surface.pending_resize.map(|_| surface.frame);
        surface.frame = replacement;
        surface.width = backing_width;
        surface.height = backing_height;
        surface.placement = placement;
        surface.pending_resize = Some(BlueprintPendingResize {
            previous_frame,
            placement,
        });
        surface.retained_text_layers.clear();
        surface.retained_text_cursor = 0;
        surface.retained_text_rendered = false;
        surface.retained_text_backbuffer_extent = None;
        surface.retained_text_backbuffer = None;
        // FontCanvas owns a document-sized source allocation, not pixels in the
        // replaced UI4 frame. Keep it warm just like uploaded sprites; the next
        // frame merely selects a different crop/destination from the same
        // source. Dropping it here made maximize synchronously restamp every
        // glyph before the replacement frame could publish.
        surface.stamped_text_layers.clear();
        surface.stamped_text_cursor = 0;
        surface.stamped_text_pending = None;
        surface.stamped_text_rendered = false;
        superseded
    };

    if let Some(superseded) = superseded
        && let Err(error) = destroy_frame(superseded)
        && error == FramePoolError::Busy
    {
        RETIRED_FRAMES.lock().push(superseded);
    }
    crate::log_info!(
        target: "ui4/blueprint-frame";
        "frame resize staged owner={:?} window={} logical_extent={}x{} backing_extent={}x{} presentation={} old_frame={} replacement_frame={} commit=after-first-released-publish\n",
        owner,
        window_id,
        width,
        height,
        backing_width,
        backing_height,
        if (backing_width, backing_height) == (width, height) {
            "1:1"
        } else {
            "direct-plane-2x"
        },
        previous_frame.raw(),
        replacement.raw(),
    );
    0
}

/// Copy one complete, tightly packed opaque RGBA8 image into the active UI4
/// write lease. Opaque input is already valid premultiplied RGBA.
pub unsafe extern "C" fn trueos_cabi_ui4_scene_frame_write_opaque_rgba8(
    window_id: u32,
    rgba_ptr: *const u8,
    rgba_len: usize,
) -> i32 {
    if rgba_ptr.is_null() || rgba_len == 0 || rgba_len & 3 != 0 {
        return ERROR_INVALID;
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let bytes = unsafe { core::slice::from_raw_parts(rgba_ptr, rgba_len) };
        let chunk_cap = trueos_vm::vmcall::PAYLOAD_CAP & !3;
        let mut offset = 0usize;
        while offset < bytes.len() {
            let end = core::cmp::min(offset.saturating_add(chunk_cap), bytes.len());
            let rc = guest_status(
                trueos_vm::vmcall::OP_BP_UI4_SCENE_WRITE_OPAQUE_RGBA8,
                window_id as u64,
                offset as u64,
                &bytes[offset..end],
            );
            if rc != 0 {
                return rc;
            }
            offset = end;
        }
        return 0;
    }
    let Some(owner) = blueprint_owner() else {
        return ERROR_CONTEXT;
    };
    let bytes = unsafe { core::slice::from_raw_parts(rgba_ptr, rgba_len) };
    write_opaque_rgba8_chunk(owner, window_id, 0, bytes)
}

/// Retain one tightly packed, straight-alpha RGBA8 sprite source for this
/// frame. The allocation remains warm across publishes and is released with
/// the owning UI4 window session.
pub unsafe extern "C" fn trueos_cabi_ui4_scene_sprite_upload_rgba8(
    window_id: u32,
    sprite_id: u32,
    width: u32,
    height: u32,
    data_ptr: *const u8,
    data_len: usize,
) -> i32 {
    let Some(expected) = expected_rgba8_len(width, height) else {
        return ERROR_INVALID;
    };
    if sprite_id == 0 || data_ptr.is_null() || data_len != expected {
        return ERROR_INVALID;
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let mut extent = [0u8; 8];
        extent[..4].copy_from_slice(&width.to_le_bytes());
        extent[4..].copy_from_slice(&height.to_le_bytes());
        let begin = guest_status(
            trueos_vm::vmcall::OP_BP_UI4_SCENE_SPRITE_UPLOAD_BEGIN,
            window_id as u64,
            sprite_id as u64,
            &extent,
        );
        if begin != 0 {
            return begin;
        }
        let bytes = unsafe { core::slice::from_raw_parts(data_ptr, expected) };
        let chunk_cap = trueos_vm::vmcall::PAYLOAD_CAP & !3;
        let mut offset = 0usize;
        while offset < bytes.len() {
            let end = core::cmp::min(offset.saturating_add(chunk_cap), bytes.len());
            let sprite_and_offset = ((sprite_id as u64) << 32) | offset as u64;
            let rc = guest_status(
                trueos_vm::vmcall::OP_BP_UI4_SCENE_SPRITE_UPLOAD_CHUNK,
                window_id as u64,
                sprite_and_offset,
                &bytes[offset..end],
            );
            if rc != 0 {
                return rc;
            }
            offset = end;
        }
        return guest_status(
            trueos_vm::vmcall::OP_BP_UI4_SCENE_SPRITE_UPLOAD_FINISH,
            window_id as u64,
            sprite_id as u64,
            &[],
        );
    }
    let Some(owner) = blueprint_owner() else {
        return ERROR_CONTEXT;
    };
    let begin = begin_sprite_rgba8_upload(owner, window_id, sprite_id, width, height);
    if begin != 0 {
        return begin;
    }
    let bytes = unsafe { core::slice::from_raw_parts(data_ptr, expected) };
    let write = write_sprite_rgba8_upload_chunk(owner, window_id, sprite_id, 0, bytes);
    if write != 0 {
        return write;
    }
    finish_sprite_rgba8_upload(owner, window_id, sprite_id)
}

/// Submit one ordered sprite/solid scene into the active sprite-frame lease.
/// Large scenes are transported in bounded chunks and rendered as consecutive
/// hardware worklists without exposing GPU addresses to the Blueprint. GuC
/// admission and completion use UI4's isolated asynchronous timeline; this
/// legacy one-call ABI returns success only after the final exact-allocation
/// producer release exists.
pub unsafe extern "C" fn trueos_cabi_ui4_scene_sprite_quads(
    window_id: u32,
    quads: *const TrueosUi4SpriteQuad,
    quad_count: usize,
) -> i32 {
    if quad_count > MAX_SPRITE_QUADS || (quads.is_null() && quad_count != 0) {
        return ERROR_INVALID;
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let begin = guest_status(
            trueos_vm::vmcall::OP_BP_UI4_SCENE_SPRITE_DRAW_BEGIN,
            window_id as u64,
            quad_count as u64,
            &[],
        );
        if begin != 0 {
            return begin;
        }
        let record_bytes = core::mem::size_of::<TrueosUi4SpriteQuad>();
        let records_per_chunk = (trueos_vm::vmcall::PAYLOAD_CAP / record_bytes).max(1);
        let input: &[TrueosUi4SpriteQuad] = if quad_count == 0 {
            &[]
        } else {
            unsafe { core::slice::from_raw_parts(quads, quad_count) }
        };
        let mut offset = 0usize;
        while offset < input.len() {
            let end = core::cmp::min(offset.saturating_add(records_per_chunk), input.len());
            let byte_len = (end - offset) * record_bytes;
            let bytes = unsafe {
                core::slice::from_raw_parts(input.as_ptr().add(offset).cast::<u8>(), byte_len)
            };
            let rc = guest_status(
                trueos_vm::vmcall::OP_BP_UI4_SCENE_SPRITE_DRAW_CHUNK,
                window_id as u64,
                offset as u64,
                bytes,
            );
            if rc != 0 {
                return rc;
            }
            offset = end;
        }
        return guest_status(
            trueos_vm::vmcall::OP_BP_UI4_SCENE_SPRITE_DRAW_FINISH,
            window_id as u64,
            0,
            &[],
        );
    }
    let Some(owner) = blueprint_owner() else {
        return ERROR_CONTEXT;
    };
    let begin = begin_sprite_scene(owner, window_id, quad_count);
    if begin != 0 {
        return begin;
    }
    if quad_count != 0 {
        let input = unsafe { core::slice::from_raw_parts(quads, quad_count) };
        let append = append_sprite_scene(owner, window_id, 0, input);
        if append != 0 {
            return append;
        }
    }
    finish_sprite_scene(owner, window_id)
}

/// Upload one tightly packed RGB565 equirectangular source owned by this UI4
/// frame. The guest transport is chunked; the retained host allocation is
/// released with the frame session.
pub unsafe extern "C" fn trueos_cabi_ui4_scene_skybox_upload_rgb565(
    window_id: u32,
    width: u32,
    height: u32,
    data_ptr: *const u8,
    data_len: usize,
) -> i32 {
    let Some(expected) = expected_rgb565_len(width, height) else {
        return ERROR_INVALID;
    };
    if data_ptr.is_null() || data_len != expected {
        return ERROR_INVALID;
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let begin = guest_status(
            trueos_vm::vmcall::OP_BP_UI4_SCENE_SKYBOX_UPLOAD_BEGIN,
            window_id as u64,
            pack_u32_pair(width, height),
            &[],
        );
        if begin != 0 {
            return begin;
        }
        let bytes = unsafe { core::slice::from_raw_parts(data_ptr, expected) };
        let mut offset = 0usize;
        while offset < bytes.len() {
            let end =
                core::cmp::min(offset.saturating_add(trueos_vm::vmcall::PAYLOAD_CAP), bytes.len());
            let rc = guest_status(
                trueos_vm::vmcall::OP_BP_UI4_SCENE_SKYBOX_UPLOAD_CHUNK,
                window_id as u64,
                offset as u64,
                &bytes[offset..end],
            );
            if rc != 0 {
                return rc;
            }
            offset = end;
        }
        return guest_status(
            trueos_vm::vmcall::OP_BP_UI4_SCENE_SKYBOX_UPLOAD_FINISH,
            window_id as u64,
            0,
            &[],
        );
    }
    let Some(owner) = blueprint_owner() else {
        return ERROR_CONTEXT;
    };
    let begin = begin_skybox_rgb565_upload(owner, window_id, width, height);
    if begin != 0 {
        return begin;
    }
    let bytes = unsafe { core::slice::from_raw_parts(data_ptr, expected) };
    let write = write_skybox_rgb565_upload_chunk(owner, window_id, 0, bytes);
    if write != 0 {
        return write;
    }
    finish_skybox_rgb565_upload(owner, window_id)
}

/// Run the existing GPGPU RGB565 skybox sampler into the active UI4 back
/// buffer. Presentation remains a separate coherent UI4 publish operation.
pub unsafe extern "C" fn trueos_cabi_ui4_scene_skybox_render_rgb565(
    window_id: u32,
    params: *const TrueosUi4SkyboxRenderParams,
) -> i32 {
    if params.is_null() {
        return ERROR_INVALID;
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let payload = unsafe {
            core::slice::from_raw_parts(
                params.cast::<u8>(),
                core::mem::size_of::<TrueosUi4SkyboxRenderParams>(),
            )
        };
        return guest_status(
            trueos_vm::vmcall::OP_BP_UI4_SCENE_SKYBOX_RENDER,
            window_id as u64,
            0,
            payload,
        );
    }
    let params = unsafe { *params };
    if !valid_skybox_render_params(params) {
        return ERROR_INVALID;
    }
    let Some(owner) = blueprint_owner() else {
        return ERROR_CONTEXT;
    };
    let (lease, skybox, frame_width, frame_height, cadence) = {
        let surfaces = SURFACES.lock();
        let Some(surface) = surfaces
            .iter()
            .find(|surface| surface.owner == owner && surface.window.raw() == window_id)
        else {
            return ERROR_NOT_FOUND;
        };
        if surface.gpu_submission_unretired {
            return ERROR_BUSY;
        }
        let Some(lease) = surface.write_lease else {
            return ERROR_STATE;
        };
        let Some(skybox) = surface.skybox else {
            return ERROR_STATE;
        };
        (lease, skybox, surface.width, surface.height, surface.cadence)
    };
    // Streaming skyboxes skip the CPU clear at frame-begin, which is safe only
    // when the shader overwrites the complete allocation.
    if cadence == FrameCadence::Streaming
        && (params.rect_x != 0
            || params.rect_y != 0
            || params.rect_width != frame_width
            || params.rect_height != frame_height)
    {
        return ERROR_INVALID;
    }
    let Ok(destination) = gpgpu_rgba_surface(lease) else {
        return ERROR_UI4;
    };
    let rendered = skybox_sample_rgb565_to_rgba8(
        skybox.surface,
        destination,
        SkyboxSampleRgb565Params {
            sky_gpu: 0,
            dst_gpu: 0,
            sky_pitch_bytes: 0,
            sky_width: 0,
            sky_height: 0,
            dst_pitch_bytes: 0,
            dst_width: 0,
            dst_height: 0,
            rect_x: params.rect_x,
            rect_y: params.rect_y,
            rect_width: params.rect_width,
            rect_height: params.rect_height,
            right_x: params.right_x,
            right_y: params.right_y,
            right_z: params.right_z,
            up_x: params.up_x,
            up_y: params.up_y,
            up_z: params.up_z,
            forward_x: params.forward_x,
            forward_y: params.forward_y,
            forward_z: params.forward_z,
            aspect_tan_half_fov_y: params.aspect_tan_half_fov_y,
            tan_half_fov_y: params.tan_half_fov_y,
        },
    );
    let mut surfaces = SURFACES.lock();
    let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
        return ERROR_NOT_FOUND;
    };
    if surface.write_lease != Some(lease) {
        return ERROR_STATE;
    }
    if rendered.ok {
        let Some(release) = rendered.release else {
            return ERROR_UI4;
        };
        surface.pending_gpu_release = Some(release);
        return 0;
    }
    if rendered.submitted {
        surface.gpu_submission_unretired = true;
        crate::log_error!(target: "ui4/blueprint-frame";
            "skybox producer quarantined owner={:?} window={} frame={} buffer={} marker=0x{:08X} submit_ms={} reason=accepted-submission-not-retired action=no-cpu-fallback+retain-source+retain-ring\n",
            owner,
            window_id,
            lease.frame.raw(),
            lease.buffer_index,
            rendered.marker,
            rendered.submit_ms,
        );
        ERROR_BUSY
    } else {
        ERROR_UI4
    }
}

/// Advance and render one full ParticleCraft frame into the active UI4 lease.
///
/// The per-window state allocation persists across frames. The call is
/// synchronous through the final GPU marker; after an accepted timeout both
/// the destination and state are quarantined and CPU fallback is forbidden.
/// Rendering keeps a fixed 640x400 logical workload and covers the current
/// frame extent, including a maximized replacement frame.
pub unsafe extern "C" fn trueos_cabi_ui4_scene_particle_craft_render(
    window_id: u32,
    params: *const TrueosUi4ParticleCraftParamsV1,
) -> i32 {
    if params.is_null() {
        return ERROR_INVALID;
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let payload = unsafe {
            core::slice::from_raw_parts(
                params.cast::<u8>(),
                core::mem::size_of::<TrueosUi4ParticleCraftParamsV1>(),
            )
        };
        return guest_status(
            trueos_vm::vmcall::OP_BP_UI4_SCENE_PARTICLE_CRAFT_RENDER,
            window_id as u64,
            0,
            payload,
        );
    }
    let wire = unsafe { *params };
    let params = ParticleCraftParamsV1 {
        version: wire.version,
        flags: wire.flags,
        seed: wire.seed,
        active_count: wire.active_count,
        dt_seconds: wire.dt_seconds,
        time_seconds: wire.time_seconds,
        emitter_x: wire.emitter_x,
        emitter_y: wire.emitter_y,
        attractor_x: wire.attractor_x,
        attractor_y: wire.attractor_y,
        attraction: wire.attraction,
        swirl: wire.swirl,
        gravity_x: wire.gravity_x,
        gravity_y: wire.gravity_y,
        drag: wire.drag,
        intensity: wire.intensity,
    };
    if !params.is_valid() {
        return ERROR_INVALID;
    }
    let Some(owner) = blueprint_owner() else {
        return ERROR_CONTEXT;
    };
    let (lease, mut craft) = {
        let mut surfaces = SURFACES.lock();
        let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
            return ERROR_NOT_FOUND;
        };
        if surface.cadence != FrameCadence::Streaming {
            return ERROR_INVALID;
        }
        if surface.gpu_submission_unretired {
            return ERROR_BUSY;
        }
        let Some(lease) = surface.write_lease else {
            return ERROR_STATE;
        };
        let craft = match surface.particle_craft.take() {
            Some(craft) => craft,
            None => match GpgpuOwnedParticleCraftState::allocate() {
                Some(craft) => craft,
                None => return ERROR_UI4,
            },
        };
        (lease, craft)
    };
    let Ok(destination) = gpgpu_rgba_surface(lease) else {
        let mut surfaces = SURFACES.lock();
        if let Some(surface) = surface_mut(&mut surfaces, owner, window_id) {
            surface.particle_craft = Some(craft);
        }
        return ERROR_UI4;
    };
    let rendered = particle_craft_rgba8_frame(&mut craft, destination, params);
    let mut surfaces = SURFACES.lock();
    let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
        return ERROR_NOT_FOUND;
    };
    if surface.write_lease != Some(lease) {
        surface.particle_craft = Some(craft);
        return ERROR_STATE;
    }
    surface.particle_craft = Some(craft);
    if rendered.ok {
        let Some(release) = rendered.release else {
            return ERROR_UI4;
        };
        surface.pending_gpu_release = Some(release);
        return 0;
    }
    if rendered.submitted {
        surface.gpu_submission_unretired = true;
        crate::log_error!(target: "ui4/blueprint-frame";
            "ParticleCraft producer quarantined owner={:?} window={} frame={} buffer={} marker=0x{:08X} submit_ms={} reason=accepted-submission-not-retired action=no-cpu-fallback+retain-state+retain-ring\n",
            owner,
            window_id,
            lease.frame.raw(),
            lease.buffer_index,
            rendered.marker,
            rendered.submit_ms,
        );
        ERROR_BUSY
    } else {
        let _ = mark_frame_buffer_cpu_authored(lease);
        ERROR_UI4
    }
}

/// Execute one reviewed ShaderToy artifact over the complete visual-mode UI4
/// back buffer. This provisional boundary intentionally accepts no source,
/// SPIR-V, Zebin, pointers, or GPU virtual addresses from the Blueprint.
pub unsafe extern "C" fn trueos_cabi_ui4_scene_shadertoy_render(
    window_id: u32,
    params: *const TrueosUi4ShadertoyParamsV1,
) -> i32 {
    if params.is_null() {
        return ERROR_INVALID;
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let payload = unsafe {
            core::slice::from_raw_parts(
                params.cast::<u8>(),
                core::mem::size_of::<TrueosUi4ShadertoyParamsV1>(),
            )
        };
        return guest_status(
            trueos_vm::vmcall::OP_BP_UI4_SCENE_SHADERTOY_RENDER,
            window_id as u64,
            0,
            payload,
        );
    }
    let wire = unsafe { *params };
    let params = ShaderToyFrameParams {
        version: wire.version,
        shader_id: wire.shader_id,
        frame: wire.frame,
        flags: wire.flags,
        time_seconds: wire.time_seconds,
        delta_seconds: wire.delta_seconds,
        frame_rate: wire.frame_rate,
        sample_rate: wire.sample_rate,
        mouse_x: wire.mouse_x,
        mouse_y: wire.mouse_y,
        click_x: wire.click_x,
        click_y: wire.click_y,
        date_year: wire.date_year,
        date_month: wire.date_month,
        date_day: wire.date_day,
        date_seconds: wire.date_seconds,
    };
    if params.version != SHADERTOY_PARAMS_VERSION || !params.is_valid() {
        return ERROR_INVALID;
    }
    let Some(owner) = blueprint_owner() else {
        return ERROR_CONTEXT;
    };
    let lease = {
        let mut surfaces = SURFACES.lock();
        let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
            return ERROR_NOT_FOUND;
        };
        if surface.visual_cadence.is_none() {
            return ERROR_INVALID;
        }
        if surface.gpu_submission_unretired {
            return ERROR_BUSY;
        }
        let Some(lease) = surface.write_lease else {
            return ERROR_STATE;
        };
        lease
    };
    let Ok(destination) = gpgpu_rgba_surface(lease) else {
        return ERROR_UI4;
    };
    let rendered = shadertoy_rgba8_surface_full(destination, params);
    let mut surfaces = SURFACES.lock();
    let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
        return ERROR_NOT_FOUND;
    };
    if surface.write_lease != Some(lease) {
        return ERROR_STATE;
    }
    if rendered.ok {
        let Some(release) = rendered.release else {
            return ERROR_UI4;
        };
        surface.pending_gpu_release = Some(release);
        return 0;
    }
    if rendered.submitted {
        surface.gpu_submission_unretired = true;
        crate::log_error!(target: "ui4/blueprint-frame";
            "ShaderToy producer quarantined owner={:?} window={} shader={} frame={} buffer={} marker=0x{:08X} submit_ms={} reason=accepted-submission-not-retired action=no-cpu-fallback+retain-ring\n",
            owner,
            window_id,
            params.shader_id,
            lease.frame.raw(),
            lease.buffer_index,
            rendered.marker,
            rendered.submit_ms,
        );
        ERROR_BUSY
    } else {
        ERROR_UI4
    }
}

/// Render one positioned text-row job through the kernel font service and
/// blend its coverage into the currently acquired UI4 frame.
pub unsafe extern "C" fn trueos_cabi_ui4_solara_text_rows(
    window_id: u32,
    font_id: u32,
    native_scale: u32,
    dst_x: i32,
    dst_y: i32,
    rgba: u32,
    rows: *const TrueosUi4SolaraTextRow,
    row_count: usize,
) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return unsafe {
            guest_text_rows(window_id, font_id, native_scale, dst_x, dst_y, rgba, rows, row_count)
        };
    }
    let Some(owner) = blueprint_owner() else {
        return ERROR_CONTEXT;
    };
    let Some(font) = GpuFontFace::from_id(font_id) else {
        return ERROR_FONT;
    };
    if let Err(reason) = ensure_font_face_available(font) {
        crate::log_warn!(target: "ui4/solara-text"; "font unavailable owner={:?} window={} font={} reason={}\n", owner, window_id, font.registry_name(), reason);
        return ERROR_FONT;
    }
    if crate::intel::render::font_native_scale_target_pixels(native_scale).is_none()
        || rows.is_null()
        || row_count == 0
        || row_count > MAX_TEXT_ROWS
    {
        return ERROR_INVALID;
    }

    // Copy all guest text before entering the font renderer. The job entries
    // below then borrow only kernel-owned, validated UTF-8 strings.
    let input = unsafe { core::slice::from_raw_parts(rows, row_count) };
    let mut strings = Vec::<String>::with_capacity(row_count);
    let mut positions = Vec::<[f32; 2]>::with_capacity(row_count);
    for row in input {
        if row.text_ptr.is_null()
            || row.text_len == 0
            || row.text_len > MAX_TEXT_ROW_BYTES
            || !row.x.is_finite()
            || !row.y.is_finite()
        {
            return ERROR_INVALID;
        }
        let bytes = unsafe { core::slice::from_raw_parts(row.text_ptr, row.text_len) };
        let Ok(text) = core::str::from_utf8(bytes) else {
            return ERROR_INVALID;
        };
        if text.chars().count() > MAX_DYNAMIC_TEXT_CHARS {
            return ERROR_INVALID;
        }
        strings.push(String::from(text));
        positions.push([row.x, row.y]);
    }
    let entries: Vec<_> = strings
        .iter()
        .zip(positions.iter())
        .map(|(text, position)| GpuFontJobEntry {
            text: GpuFontTextRequest::SingleLine(text.as_str()),
            position: *position,
            font_pixels: crate::graphics::font::FONT_TESSEL_BASE_PX,
            slant: 0.0,
        })
        .collect();
    let readback = match render_font_job_readback_once(GpuFontJob {
        entries: entries.as_slice(),
        font,
        native_scale,
    }) {
        Ok(readback) => readback,
        Err(reason) => {
            crate::log_warn!(target: "ui4/solara-text"; "text render rejected owner={:?} window={} reason={}\n", owner, window_id, reason);
            return ERROR_FONT;
        }
    };

    let result = {
        let mut surfaces = SURFACES.lock();
        let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
            recycle_font_job_readback(readback);
            return ERROR_NOT_FOUND;
        };
        let Some(lease) = surface.write_lease else {
            recycle_font_job_readback(readback);
            return ERROR_STATE;
        };
        match writable_rgba_view(lease) {
            Ok(view) => {
                blend_font_coverage(view, &readback, dst_x, dst_y, rgba);
                crate::intel::dma_flush(view.virt, view.byte_len);
                0
            }
            Err(_) => ERROR_UI4,
        }
    };
    recycle_font_job_readback(readback);
    result
}

/// Render Solara paint records in the frame's fixed pixel coordinate space.
pub unsafe extern "C" fn trueos_cabi_ui4_solara_text_scene(
    window_id: u32,
    font_id: u32,
    viewport_width: u32,
    viewport_height: u32,
    rgba: u32,
    rows: *const TrueosUi4SolaraSceneTextRow,
    row_count: usize,
) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return unsafe {
            guest_text_scene(
                window_id,
                font_id,
                viewport_width,
                viewport_height,
                rgba,
                rows,
                row_count,
            )
        };
    }
    let Some(owner) = blueprint_owner() else {
        return ERROR_CONTEXT;
    };
    let stamp_once = font_id & TEXT_SCENE_FONT_ID_STAMP_ONCE != 0;
    let backbuffer = font_id & TEXT_SCENE_FONT_ID_BACKBUFFER != 0;
    if stamp_once && backbuffer {
        return ERROR_INVALID;
    }
    let Some(font) = GpuFontFace::from_id(font_id & !TEXT_SCENE_FONT_ID_FLAGS) else {
        return ERROR_FONT;
    };
    if rows.is_null() || row_count == 0 || row_count > MAX_TEXT_ROWS {
        return ERROR_INVALID;
    }
    {
        let mut surfaces = SURFACES.lock();
        let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
            return ERROR_NOT_FOUND;
        };
        let frame_target = surface.width == viewport_width && surface.height == viewport_height;
        let valid_backbuffer = backbuffer
            && viewport_width <= TEXT_BACKBUFFER_MAX_EXTENT
            && viewport_height <= TEXT_BACKBUFFER_MAX_EXTENT;
        if !frame_target && !valid_backbuffer {
            return ERROR_INVALID;
        }
        if surface.write_lease.is_none() {
            return ERROR_STATE;
        }
    }

    let input = unsafe { core::slice::from_raw_parts(rows, row_count) };
    let mut runs = Vec::<RetainedFontRun>::with_capacity(row_count);
    for row in input {
        if row.text_ptr.is_null()
            || row.text_len == 0
            || row.text_len > MAX_TEXT_ROW_BYTES
            || !row.x.is_finite()
            || !row.y.is_finite()
            || !row.font_pixels.is_finite()
            || row.font_pixels <= 0.0
            || row.font_pixels > 256.0
        {
            return ERROR_INVALID;
        }
        let bytes = unsafe { core::slice::from_raw_parts(row.text_ptr, row.text_len) };
        let Ok(text) = core::str::from_utf8(bytes) else {
            return ERROR_INVALID;
        };
        if text.chars().count() > MAX_DYNAMIC_TEXT_CHARS {
            return ERROR_INVALID;
        }
        runs.push(RetainedFontRun {
            text: String::from(text),
            position: [row.x, row.y],
            font_pixels: row.font_pixels,
            slant: 0.0,
        });
    }
    if stamp_once {
        stamp_scene_entries_for_surface(
            owner,
            window_id,
            font,
            viewport_width,
            viewport_height,
            rgba,
            runs,
        )
    } else {
        retain_scene_entries_for_surface(
            owner,
            window_id,
            font,
            viewport_width,
            viewport_height,
            rgba,
            runs,
            backbuffer,
        )
    }
}

/// Build or replace one persistent transparent RGBA8 font canvas.
///
/// Unlike the legacy retained-text scene, this operation does not acquire a
/// UI4 frame lease. FontKernel owns the asynchronous stamp until one complete
/// RGBA8 allocation is ready, so a Blueprint can safely wait without wedging
/// presentation. Equal-color rows are grouped only inside the kernel request;
/// the retained consumer object is always one canvas.
pub unsafe extern "C" fn trueos_cabi_ui4_font_canvas(
    window_id: u32,
    font_id: u32,
    canvas_width: u32,
    canvas_height: u32,
    rows: *const TrueosUi4FontCanvasRow,
    row_count: usize,
) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return unsafe {
            guest_font_canvas(window_id, font_id, canvas_width, canvas_height, rows, row_count)
        };
    }
    let Some(owner) = blueprint_owner() else {
        return ERROR_CONTEXT;
    };
    let Some(font) = GpuFontFace::from_id(font_id) else {
        return ERROR_FONT;
    };
    if rows.is_null()
        || row_count == 0
        || row_count > MAX_FONT_CANVAS_ROWS
        || canvas_width == 0
        || canvas_height == 0
        || canvas_width > TEXT_BACKBUFFER_MAX_EXTENT
        || canvas_height > TEXT_BACKBUFFER_MAX_EXTENT
    {
        return ERROR_INVALID;
    }

    let input = unsafe { core::slice::from_raw_parts(rows, row_count) };
    let mut owned_rows = Vec::with_capacity(row_count);
    let mut glyphs = 0usize;
    for row in input {
        if row.text_ptr.is_null()
            || row.text_len == 0
            || row.text_len > MAX_TEXT_ROW_BYTES
            || !row.x.is_finite()
            || !row.y.is_finite()
            || !row.font_pixels.is_finite()
            || row.font_pixels <= 0.0
            || row.font_pixels > 256.0
        {
            return ERROR_INVALID;
        }
        let bytes = unsafe { core::slice::from_raw_parts(row.text_ptr, row.text_len) };
        let Ok(text) = core::str::from_utf8(bytes) else {
            return ERROR_INVALID;
        };
        glyphs = glyphs.saturating_add(text.chars().count());
        if glyphs > TEXT_BACKBUFFER_MAX_GLYPHS {
            return ERROR_INVALID;
        }
        owned_rows.push(BlueprintFontCanvasRow {
            text: String::from(text),
            position: [row.x, row.y],
            font_pixels: row.font_pixels,
            color_rgba: row.color_rgba,
        });
    }
    let description = BlueprintFontCanvasDescription {
        font,
        width: canvas_width,
        height: canvas_height,
        rows: owned_rows,
    };
    if font_canvas_internal_layer_count(&description.rows) > MAX_FONT_CANVAS_INTERNAL_LAYERS {
        return ERROR_INVALID;
    }
    retain_font_canvas_for_surface(owner, window_id, description)
}

fn font_canvas_internal_layer_count(rows: &[BlueprintFontCanvasRow]) -> usize {
    let mut groups = Vec::<(u32, usize)>::new();
    for row in rows {
        if let Some((_, count)) = groups.iter_mut().find(|(color_rgba, count)| {
            *color_rgba == row.color_rgba && *count < MAX_FONT_CANVAS_RUNS_PER_LAYER
        }) {
            *count += 1;
        } else {
            groups.push((row.color_rgba, 1));
        }
    }
    groups.len()
}

fn font_canvas_request(description: &BlueprintFontCanvasDescription) -> FontStampRequest {
    use trueos_helio_runtime::picasso_scene::{Color, FontFace, FontLookupRun, FontSlant, Rect};

    let face = match description.font {
        GpuFontFace::Default => FontFace::Default,
        GpuFontFace::NotoSansSc => FontFace::NotoSansSc,
        GpuFontFace::Inconsolata => FontFace::Inconsolata,
    };
    let lookup_rows = description
        .rows
        .iter()
        .map(|row| {
            let [red, green, blue, alpha] = row.color_rgba.to_le_bytes();
            FontLookupRun {
                rect: Rect::new(0.0, 0.0, description.width as f32, description.height as f32),
                origin: row.position,
                text: row.text.clone(),
                face,
                slant: FontSlant::Normal,
                font_pixels: row.font_pixels,
                color: Color::rgba(red, green, blue, alpha),
            }
        })
        .collect::<Vec<_>>();
    if let Ok(request) = crate::r::font_kernel_service::picasso_font_lookup_canvas_request(
        lookup_rows.as_slice(),
        description.width,
        description.height,
        description.width,
        description.height,
    ) {
        return request;
    }

    // Compatibility safety net for an already-validated legacy description.
    // Native Picasso lookup integration uses the typed path above; retaining
    // this construction means an optional cache/grouping optimization can
    // never turn a formerly valid visual canvas into a frame failure.
    let mut groups = Vec::<(u32, Vec<RetainedFontRun>)>::new();
    for row in &description.rows {
        let group_index = match groups.iter_mut().position(|(color_rgba, runs)| {
            *color_rgba == row.color_rgba && runs.len() < MAX_FONT_CANVAS_RUNS_PER_LAYER
        }) {
            Some(index) => index,
            None => {
                groups.push((row.color_rgba, Vec::new()));
                groups.len() - 1
            }
        };
        groups[group_index].1.push(RetainedFontRun {
            text: row.text.clone(),
            position: row.position,
            font_pixels: row.font_pixels,
            slant: 0.0,
        });
    }
    let layers = groups
        .into_iter()
        .map(|(color_rgba, runs)| {
            let [r, g, b, a] = color_rgba.to_le_bytes();
            FontStampLayer {
                scene: RetainSceneRequest {
                    runs,
                    font: description.font,
                    viewport_width: description.width,
                    viewport_height: description.height,
                    raster_width: description.width,
                    raster_height: description.height,
                    positioning: RetainedFontPositioning::SceneOrigin,
                },
                foreground: GpuFontRgba::new(r, g, b, a),
            }
        })
        .collect();
    debug_assert!(
        font_canvas_internal_layer_count(&description.rows) <= MAX_FONT_CANVAS_INTERNAL_LAYERS
    );
    FontStampRequest {
        layers,
        fit: FontStampFit::Canvas,
    }
}

fn retain_font_canvas_for_surface(
    owner: WindowOwner,
    window_id: u32,
    description: BlueprintFontCanvasDescription,
) -> i32 {
    let mut surfaces = SURFACES.lock();
    let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
        return ERROR_NOT_FOUND;
    };
    if surface.gpu_submission_unretired {
        return ERROR_BUSY;
    }

    if let Some(canvas) = surface.font_canvas.as_mut() {
        if !canvas.description.same_canvas(&description) {
            if canvas.pending.is_some() {
                return ERROR_BUSY;
            }
            let pending = match submit_stamp(font_canvas_request(&description)) {
                Ok(pending) => pending,
                Err(FontKernelError::QueueFull) => return ERROR_BUSY,
                Err(error) => {
                    log_font_kernel_error(
                        "canvas-stamp",
                        owner,
                        window_id,
                        description.font,
                        description.rows.len(),
                        error,
                    );
                    return ERROR_FONT;
                }
            };
            *canvas = BlueprintFontCanvas {
                description,
                pending: Some(pending),
                ready: None,
                submitted_ms: Instant::now().as_millis(),
            };
        }
    } else {
        let pending = match submit_stamp(font_canvas_request(&description)) {
            Ok(pending) => pending,
            Err(FontKernelError::QueueFull) => return ERROR_BUSY,
            Err(error) => {
                log_font_kernel_error(
                    "canvas-stamp",
                    owner,
                    window_id,
                    description.font,
                    description.rows.len(),
                    error,
                );
                return ERROR_FONT;
            }
        };
        surface.font_canvas = Some(BlueprintFontCanvas {
            description,
            pending: Some(pending),
            ready: None,
            submitted_ms: Instant::now().as_millis(),
        });
    }

    let canvas = surface.font_canvas.as_mut().expect("font canvas installed");
    if canvas.ready.is_some() {
        return 0;
    }
    let completion = canvas.pending.as_mut().and_then(PendingFontStamp::try_take);
    let Some(completion) = completion else {
        return ERROR_BUSY;
    };
    canvas.pending = None;
    match completion {
        Ok(buffer) => {
            let ticket = buffer.ticket().raw();
            let extent = buffer.surface();
            let glyphs = buffer.glyphs();
            let submits = buffer.submits();
            let walkers = buffer.active_walkers();
            let build_ms = Instant::now()
                .as_millis()
                .saturating_sub(canvas.submitted_ms);
            canvas.ready = Some(buffer);
            crate::log_info!(
                target: "ui4/font-canvas";
                "FontKernel RGBA canvas ready owner={:?} window={} ticket={} rows={} internal_layers={} extent={}x{} glyphs={} submits={} walkers={} build_ms={} storage=gpu-vm-rgba8 alpha=premultiplied-coverage context=kernel-gpgpu-font path=skrifa->gpu-vm-r8->cpp-igc->guc-font-rcs->owned-rgba8 cpu_readback=0 cpu_frame_copy=0\n",
                owner,
                window_id,
                ticket,
                canvas.description.rows.len(),
                font_canvas_internal_layer_count(&canvas.description.rows),
                extent.width,
                extent.height,
                glyphs,
                submits,
                walkers,
                build_ms,
            );
            0
        }
        Err(error) => {
            log_font_kernel_error(
                "canvas-stamp",
                owner,
                window_id,
                canvas.description.font,
                canvas.description.rows.len(),
                error,
            );
            ERROR_FONT
        }
    }
}

fn stamp_scene_entries_for_surface(
    owner: WindowOwner,
    window_id: u32,
    font: GpuFontFace,
    viewport_width: u32,
    viewport_height: u32,
    rgba: u32,
    runs: Vec<RetainedFontRun>,
) -> i32 {
    let description = BlueprintRetainedTextDescription {
        font,
        viewport_width,
        viewport_height,
        runs,
    };
    let mut surfaces = SURFACES.lock();
    let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
        return ERROR_NOT_FOUND;
    };
    if surface.write_lease.is_none() {
        return ERROR_STATE;
    }
    if surface.gpu_submission_unretired {
        return ERROR_BUSY;
    }
    if surface.stamped_text_pending.is_some() {
        return ERROR_STATE;
    }

    let layer_index = surface.stamped_text_cursor;
    let replacement = BlueprintStampedTextLayer {
        description,
        color_rgba: rgba,
    };
    if layer_index < surface.stamped_text_layers.len() {
        surface.stamped_text_layers[layer_index] = replacement;
    } else {
        surface.stamped_text_layers.push(replacement);
    }
    surface.stamped_text_cursor = surface.stamped_text_cursor.saturating_add(1);
    0
}

fn retain_scene_entries_for_surface(
    owner: WindowOwner,
    window_id: u32,
    font: GpuFontFace,
    viewport_width: u32,
    viewport_height: u32,
    rgba: u32,
    runs: Vec<RetainedFontRun>,
    backbuffer: bool,
) -> i32 {
    let description = BlueprintRetainedTextDescription {
        font,
        viewport_width,
        viewport_height,
        runs,
    };
    let mut surfaces = SURFACES.lock();
    let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
        return ERROR_NOT_FOUND;
    };
    if surface.write_lease.is_none() {
        return ERROR_STATE;
    }
    if surface.gpu_submission_unretired {
        return ERROR_BUSY;
    }
    if backbuffer {
        let extent = (viewport_width, viewport_height);
        let glyphs = surface
            .retained_text_layers
            .iter()
            .take(surface.retained_text_cursor)
            .flat_map(|layer| layer.description.runs.iter())
            .chain(description.runs.iter())
            .fold(0usize, |total, run| total.saturating_add(run.text.chars().count()));
        if glyphs > TEXT_BACKBUFFER_MAX_GLYPHS {
            return ERROR_INVALID;
        }
        if surface.retained_text_cursor == 0 {
            surface.retained_text_backbuffer_extent = Some(extent);
            surface.retained_text_backbuffer = None;
        } else if surface.retained_text_backbuffer_extent != Some(extent) {
            return ERROR_INVALID;
        }
    } else if surface.retained_text_cursor == 0 {
        surface.retained_text_backbuffer_extent = None;
        surface.retained_text_backbuffer = None;
    } else if surface.retained_text_backbuffer_extent.is_some() {
        return ERROR_STATE;
    }

    let layer_index = surface.retained_text_cursor;
    let translation = surface
        .retained_text_layers
        .get(layer_index)
        .and_then(|layer| layer.description.translation_to(&description));

    if translation.is_none() {
        let request = RetainSceneRequest {
            runs: description.runs.clone(),
            font,
            viewport_width,
            viewport_height,
            raster_width: viewport_width,
            raster_height: viewport_height,
            positioning: RetainedFontPositioning::SceneOrigin,
        };
        let pending = match submit_retain_scene(request) {
            Ok(pending) => pending,
            Err(FontKernelError::QueueFull) => return ERROR_BUSY,
            Err(error) => {
                log_font_kernel_error(
                    "retain",
                    owner,
                    window_id,
                    font,
                    description.runs.len(),
                    error,
                );
                return ERROR_FONT;
            }
        };
        let replacement = BlueprintRetainedTextLayer {
            description,
            color_rgba: rgba,
            translation_px: [0, 0],
            state: BlueprintRetainedTextState::Pending(pending),
        };
        if layer_index < surface.retained_text_layers.len() {
            surface.retained_text_layers[layer_index] = replacement;
        } else {
            surface.retained_text_layers.push(replacement);
        }
    } else if let Some(layer) = surface.retained_text_layers.get_mut(layer_index) {
        layer.color_rgba = rgba;
        layer.translation_px = translation.unwrap_or([0, 0]);
    }

    let layer = &mut surface.retained_text_layers[layer_index];
    let completion = match &mut layer.state {
        BlueprintRetainedTextState::Pending(pending) => pending.try_take(),
        BlueprintRetainedTextState::Ready(_) | BlueprintRetainedTextState::NoCoverage => None,
    };
    if let Some(completion) = completion {
        match completion {
            Ok(scene) => {
                let ticket = match &layer.state {
                    BlueprintRetainedTextState::Pending(pending) => pending.ticket().raw(),
                    BlueprintRetainedTextState::Ready(_)
                    | BlueprintRetainedTextState::NoCoverage => 0,
                };
                let mask_count = scene.mask_count();
                layer.state = BlueprintRetainedTextState::Ready(scene);
                crate::log_info!(
                    target: "ui4/solara-text";
                    "FontKernel retained owner={:?} window={} layer={} ticket={} font={} runs={} masks={} target={}x{} storage=gpu-vm-r8-layers\n",
                    owner,
                    window_id,
                    layer_index,
                    ticket,
                    font.registry_name(),
                    layer.description.runs.len(),
                    mask_count,
                    viewport_width,
                    viewport_height,
                );
            }
            Err(error) if retained_text_error_is_no_coverage(error) => {
                let ticket = match &layer.state {
                    BlueprintRetainedTextState::Pending(pending) => pending.ticket().raw(),
                    BlueprintRetainedTextState::Ready(_)
                    | BlueprintRetainedTextState::NoCoverage => 0,
                };
                layer.state = BlueprintRetainedTextState::NoCoverage;
                crate::log_info!(
                    target: "ui4/solara-text";
                    "FontKernel retained no-coverage owner={:?} window={} layer={} ticket={} font={} runs={} target={}x{} action=transparent-noop\n",
                    owner,
                    window_id,
                    layer_index,
                    ticket,
                    font.registry_name(),
                    layer.description.runs.len(),
                    viewport_width,
                    viewport_height,
                );
            }
            Err(error) => {
                log_font_kernel_error(
                    "retain",
                    owner,
                    window_id,
                    font,
                    layer.description.runs.len(),
                    error,
                );
                surface.retained_text_layers.remove(layer_index);
                return ERROR_FONT;
            }
        }
    }
    if matches!(&layer.state, BlueprintRetainedTextState::Pending(_)) {
        return ERROR_BUSY;
    }
    surface.retained_text_cursor = surface.retained_text_cursor.saturating_add(1);
    0
}

fn log_font_kernel_error(
    operation: &'static str,
    owner: WindowOwner,
    window_id: u32,
    font: GpuFontFace,
    runs: usize,
    error: FontKernelError,
) {
    let (class, reason) = match error {
        FontKernelError::QueueFull => ("queue-full", "font-kernel-queue"),
        FontKernelError::InvalidRequest(reason) => ("invalid", reason),
        FontKernelError::Unavailable(reason) => ("unavailable", reason),
        FontKernelError::SubmittedIncomplete(reason) => ("submitted-incomplete", reason),
    };
    crate::log_warn!(
        target: "ui4/solara-text";
        "FontKernel {} failed owner={:?} window={} font={} runs={} class={} reason={}\n",
        operation,
        owner,
        window_id,
        font.registry_name(),
        runs,
        class,
        reason,
    );
}

fn retained_text_error_is_no_coverage(error: FontKernelError) -> bool {
    matches!(error, FontKernelError::Unavailable("font-coverage-empty"))
}

fn retained_text_scene(
    state: &BlueprintRetainedTextState,
) -> Result<Option<&FontKernelRetainedScene>, i32> {
    match state {
        BlueprintRetainedTextState::Pending(_) => Err(ERROR_BUSY),
        BlueprintRetainedTextState::Ready(scene) => Ok(Some(scene)),
        BlueprintRetainedTextState::NoCoverage => Ok(None),
    }
}

fn render_retained_text_for_surface(owner: WindowOwner, window_id: u32) -> i32 {
    let mut surfaces = SURFACES.lock();
    let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
        return ERROR_NOT_FOUND;
    };
    if surface.retained_text_backbuffer_extent.is_some() {
        return 0;
    }
    if surface.retained_text_rendered || surface.retained_text_cursor == 0 {
        return 0;
    }
    if surface.gpu_submission_unretired {
        return ERROR_BUSY;
    }
    let Some(lease) = surface.write_lease else {
        return ERROR_STATE;
    };
    let destination = match gpgpu_rgba_surface(lease) {
        Ok(destination) => destination,
        Err(_) => return ERROR_UI4,
    };
    let mut masks = Vec::with_capacity(surface.retained_text_cursor);
    for layer in surface
        .retained_text_layers
        .iter()
        .take(surface.retained_text_cursor)
    {
        let scene = match retained_text_scene(&layer.state) {
            Ok(Some(scene)) => scene,
            Ok(None) => continue,
            Err(error) => {
                if mark_frame_buffer_cpu_authored(lease).is_err() {
                    return ERROR_UI4;
                }
                return error;
            }
        };
        for retained_mask in scene.masks() {
            let Some((mask, origin)) = retained_mask else {
                if mark_frame_buffer_cpu_authored(lease).is_err() {
                    return ERROR_UI4;
                }
                return ERROR_FONT;
            };
            masks.push(GpgpuGlyphMaskLayer {
                mask,
                mask_rect: GpgpuRect::new(0, 0, mask.width, mask.height),
                dst_xy: GpgpuPoint::new(
                    origin[0].saturating_add(layer.translation_px[0]),
                    origin[1].saturating_add(layer.translation_px[1]),
                ),
                color_rgba: layer.color_rgba,
            });
        }
    }

    let mut submitted = false;
    let mut submits = 0usize;
    let mut active_walkers = 0usize;
    for chunk in masks.chunks(RETAINED_TEXT_MASK_BATCH_CAPACITY) {
        let rendered = glyph_mask_layers_rgba8_2d_mode(chunk, destination, false);
        submitted |= rendered.submitted;
        submits = submits.saturating_add(rendered.submits);
        active_walkers = active_walkers.saturating_add(rendered.active_walkers);
        if !rendered.ok {
            if submitted {
                surface.gpu_submission_unretired = true;
                crate::log_error!(
                    target: "ui4/solara-text";
                    "FontKernel retained batch quarantined owner={:?} window={} frame={} buffer={} layers={} action=retain-frame+resident-masks\n",
                    owner,
                    window_id,
                    lease.frame.raw(),
                    lease.buffer_index,
                    masks.len(),
                );
                return ERROR_BUSY;
            }
            if mark_frame_buffer_cpu_authored(lease).is_err() {
                return ERROR_UI4;
            }
            return ERROR_FONT;
        }
    }

    let finalizer = release_rgba8_surface_for_scanout(destination);
    if !finalizer.ok {
        if submitted || finalizer.submitted {
            surface.gpu_submission_unretired = true;
            crate::log_error!(
                target: "ui4/solara-text";
                "FontKernel retained release quarantined owner={:?} window={} frame={} buffer={} layers={} action=retain-frame+resident-masks\n",
                owner,
                window_id,
                lease.frame.raw(),
                lease.buffer_index,
                masks.len(),
            );
            return ERROR_BUSY;
        }
        if mark_frame_buffer_cpu_authored(lease).is_err() {
            return ERROR_UI4;
        }
        return ERROR_FONT;
    }
    let Some(release) = finalizer.release else {
        surface.gpu_submission_unretired = true;
        return ERROR_BUSY;
    };
    surface.pending_gpu_release = Some(release);
    surface.retained_text_rendered = true;
    crate::log_info!(
        target: "ui4/solara-text";
        "FontKernel retained scene stamped owner={:?} window={} layers={} batches={} walkers={} target={}x{} path=resident-r8-batch cpu_readback=0 cpu_frame_copy=0\n",
        owner,
        window_id,
        masks.len(),
        submits,
        active_walkers,
        surface.width,
        surface.height,
    );
    0
}

/// Materialize the retained document masks once into a persistent RGBA8 source.
///
/// The resulting allocation belongs to the Blueprint frame rather than one
/// UI4 back-buffer lease. Later pans therefore sample another source rectangle
/// without re-entering FontKernel or rebuilding the DOM text scene.
fn render_retained_text_backbuffer_for_surface(owner: WindowOwner, window_id: u32) -> i32 {
    let mut surfaces = SURFACES.lock();
    let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
        return ERROR_NOT_FOUND;
    };
    let Some((width, height)) = surface.retained_text_backbuffer_extent else {
        return 0;
    };
    if surface.retained_text_backbuffer.is_some() {
        return 0;
    }
    if surface.retained_text_cursor == 0 || surface.write_lease.is_none() {
        return ERROR_STATE;
    }
    if surface.gpu_submission_unretired {
        return ERROR_BUSY;
    }

    let mut masks = Vec::with_capacity(surface.retained_text_cursor);
    for layer in surface
        .retained_text_layers
        .iter()
        .take(surface.retained_text_cursor)
    {
        let scene = match retained_text_scene(&layer.state) {
            Ok(Some(scene)) => scene,
            Ok(None) => continue,
            Err(error) => return error,
        };
        for retained_mask in scene.masks() {
            let Some((mask, origin)) = retained_mask else {
                return ERROR_FONT;
            };
            masks.push(GpgpuGlyphMaskLayer {
                mask,
                mask_rect: GpgpuRect::new(0, 0, mask.width, mask.height),
                dst_xy: GpgpuPoint::new(origin[0], origin[1]),
                color_rgba: layer.color_rgba,
            });
        }
    }
    let Some(clear_rgba) = surface.sprite_clear_rgba else {
        return ERROR_STATE;
    };
    let [r, g, b, a] = clear_rgba.to_le_bytes();
    let premultiplied_clear =
        u32::from_le_bytes(PremultipliedRgba8::from_straight_rgba(r, g, b, a).to_native_bytes());
    let Some(storage) =
        allocate_font_instance_rgba8_surface_cleared(width, height, premultiplied_clear)
    else {
        return ERROR_UI4;
    };
    let destination = storage.surface();
    surface.retained_text_backbuffer = Some(storage);

    let mut submits = 0usize;
    let mut active_walkers = 0usize;
    for chunk in masks.chunks(RETAINED_TEXT_MASK_BATCH_CAPACITY) {
        let rendered = glyph_mask_layers_rgba8_2d_mode(chunk, destination, false);
        submits = submits.saturating_add(rendered.submits);
        active_walkers = active_walkers.saturating_add(rendered.active_walkers);
        if !rendered.ok {
            if rendered.submitted {
                surface.gpu_submission_unretired = true;
                crate::log_error!(
                    target: "ui4/solara-text";
                    "FontKernel document backbuffer quarantined owner={:?} window={} layers={} target={}x{} action=retain-rgba+resident-masks\n",
                    owner,
                    window_id,
                    masks.len(),
                    width,
                    height,
                );
                return ERROR_UI4;
            }
            surface.retained_text_backbuffer = None;
            return ERROR_FONT;
        }
    }

    surface.retained_text_rendered = true;
    crate::log_info!(
        target: "ui4/solara-text";
        "FontKernel document backbuffer ready owner={:?} window={} layers={} batches={} walkers={} target={}x{} storage=gpu-vm-rgba8 path=resident-r8-batch cpu_readback=0 cpu_frame_copy=0\n",
        owner,
        window_id,
        masks.len(),
        submits,
        active_walkers,
        width,
        height,
    );
    surface.retained_text_layers.clear();
    surface.retained_text_cursor = 0;
    0
}

fn render_stamped_text_for_surface(owner: WindowOwner, window_id: u32) -> i32 {
    let mut surfaces = SURFACES.lock();
    let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
        return ERROR_NOT_FOUND;
    };
    if surface.stamped_text_rendered || surface.stamped_text_cursor == 0 {
        return 0;
    }
    if surface.gpu_submission_unretired {
        return ERROR_BUSY;
    }
    let Some(lease) = surface.write_lease else {
        return ERROR_STATE;
    };
    let destination = match gpgpu_rgba_surface(lease) {
        Ok(destination) => destination,
        Err(_) => return ERROR_UI4,
    };
    if surface.stamped_text_pending.is_none() {
        let layers = surface
            .stamped_text_layers
            .iter()
            .take(surface.stamped_text_cursor)
            .map(|layer| {
                let [r, g, b, a] = layer.color_rgba.to_le_bytes();
                FontStampLayer {
                    scene: RetainSceneRequest {
                        runs: layer.description.runs.clone(),
                        font: layer.description.font,
                        viewport_width: layer.description.viewport_width,
                        viewport_height: layer.description.viewport_height,
                        raster_width: layer.description.viewport_width,
                        raster_height: layer.description.viewport_height,
                        positioning: RetainedFontPositioning::SceneOrigin,
                    },
                    foreground: GpuFontRgba::new(r, g, b, a),
                }
            })
            .collect();
        let request = FontStampRequest {
            layers,
            fit: FontStampFit::Canvas,
        };
        surface.stamped_text_pending = match submit_frame_stamp(request, destination) {
            Ok(pending) => Some(pending),
            Err(FontKernelError::QueueFull) => return ERROR_BUSY,
            Err(error) => {
                log_font_kernel_error(
                    "frame-stamp",
                    owner,
                    window_id,
                    surface.stamped_text_layers[0].description.font,
                    surface.stamped_text_cursor,
                    error,
                );
                return ERROR_FONT;
            }
        };
    }

    let completion = surface
        .stamped_text_pending
        .as_mut()
        .and_then(PendingFontFrameStamp::try_take);
    let Some(completion) = completion else {
        return ERROR_BUSY;
    };
    surface.stamped_text_pending = None;
    let stamped = match completion {
        Ok(stamped) => stamped,
        Err(FontKernelError::SubmittedIncomplete(reason)) => {
            surface.gpu_submission_unretired = true;
            crate::log_error!(
                target: "ui4/solara-text";
                "FontKernel frame stamp quarantined owner={:?} window={} frame={} buffer={} layers={} reason={} action=retain-frame\n",
                owner,
                window_id,
                lease.frame.raw(),
                lease.buffer_index,
                surface.stamped_text_cursor,
                reason,
            );
            return ERROR_BUSY;
        }
        Err(error) => {
            log_font_kernel_error(
                "frame-stamp",
                owner,
                window_id,
                surface.stamped_text_layers[0].description.font,
                surface.stamped_text_cursor,
                error,
            );
            return ERROR_FONT;
        }
    };
    surface.pending_gpu_release = Some(stamped.release());
    surface.stamped_text_rendered = true;
    crate::log_info!(
        target: "ui4/solara-text";
        "FontKernel frame stamped owner={:?} window={} layers={} glyphs={} submits={} walkers={} target={}x{} context=kernel-gpgpu-font path=skrifa->gpu-vm-r8->cpp-igc->guc-font-rcs->ui4-frame-rgba8 cpu_readback=0 cpu_frame_copy=0 staging_rgba=0\n",
        owner,
        window_id,
        surface.stamped_text_cursor,
        stamped.glyphs(),
        stamped.submits(),
        stamped.active_walkers(),
        surface.width,
        surface.height,
    );
    0
}

/// Publish the completed dirty buffer and its window damage.
pub extern "C" fn trueos_cabi_ui4_solara_frame_publish(
    window_id: u32,
    damage_x: u32,
    damage_y: u32,
    damage_width: u32,
    damage_height: u32,
) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let mut payload = [0u8; 8];
        payload[..4].copy_from_slice(&damage_width.to_le_bytes());
        payload[4..].copy_from_slice(&damage_height.to_le_bytes());
        return guest_status(
            trueos_vm::vmcall::OP_BP_UI4_SOLARA_FRAME_PUBLISH,
            window_id as u64,
            pack_u32_pair(damage_x, damage_y),
            &payload,
        );
    }
    let Some(owner) = blueprint_owner() else {
        return ERROR_CONTEXT;
    };
    if damage_width == 0 || damage_height == 0 {
        return ERROR_INVALID;
    }
    let retained_text = render_retained_text_for_surface(owner, window_id);
    if retained_text != 0 {
        return retained_text;
    }
    let stamped_text = render_stamped_text_for_surface(owner, window_id);
    if stamped_text != 0 {
        return stamped_text;
    }
    let mut surfaces = SURFACES.lock();
    let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
        return ERROR_NOT_FOUND;
    };
    if damage_x >= surface.width || damage_y >= surface.height {
        return ERROR_INVALID;
    }
    let damage = DamageRect {
        x: damage_x,
        y: damage_y,
        width: damage_width.min(surface.width - damage_x),
        height: damage_height.min(surface.height - damage_y),
    };
    let Some(lease) = surface.write_lease.take() else {
        return ERROR_STATE;
    };
    if surface.gpu_submission_unretired {
        surface.write_lease = Some(lease);
        return ERROR_BUSY;
    }
    let compute_release = surface.pending_gpu_release.take();
    let render_release = surface.pending_render_release.take();
    let publish = match (compute_release, render_release) {
        (Some(release), None) => publish_gpgpu_scene_frame_buffer(lease, release),
        (None, Some(release)) => publish_resident_scene_frame_buffer(lease, release),
        (None, None) => publish_frame_buffer(lease),
        (Some(_), Some(_)) => Err(super::FramePoolError::ProducerReleaseRequired),
    };
    if let Err(error) = publish {
        crate::log_warn!(
            target: "ui4/blueprint-frame";
            "frame publish handoff rejected owner={:?} window={} frame={} buffer={} cadence={:?} gpu_release={} error={:?} action=retain-write-lease\n",
            owner,
            window_id,
            lease.frame.raw(),
            lease.buffer_index,
            surface.cadence,
            u8::from(compute_release.is_some() || render_release.is_some()),
            error,
        );
        surface.write_lease = Some(lease);
        surface.pending_gpu_release = compute_release;
        surface.pending_render_release = render_release;
        return ERROR_UI4;
    }
    if let Some(pending) = surface.pending_resize {
        if lease.frame != surface.frame
            || commit_window_frame_replacement(
                owner,
                surface.window,
                surface.frame,
                pending.placement,
                damage,
            )
            .is_err()
        {
            crate::log_warn!(target: "ui4/blueprint-frame"; "frame resize commit failed owner={:?} window={} old_frame={} replacement_frame={} action=retain-old-surflive-front\n", owner, window_id, pending.previous_frame.raw(), surface.frame.raw());
            return ERROR_UI4;
        }
        surface.pending_resize = None;
        if let Err(error) = destroy_frame(pending.previous_frame)
            && error == FramePoolError::Busy
        {
            RETIRED_FRAMES.lock().push(pending.previous_frame);
        }
        crate::log_info!(target: "ui4/blueprint-frame"; "frame resize committed owner={:?} window={} old_frame={} frame={} extent={}x{} action=atomic-frame+placement+first-publish old_release=surflive\n", owner, window_id, pending.previous_frame.raw(), surface.frame.raw(), surface.width, surface.height);
    } else {
        let immutable_replacement =
            (lease.frame != surface.frame).then_some((surface.frame, lease.frame));
        if let Some((previous, replacement)) = immutable_replacement
            && replace_window_frame(owner, surface.window, replacement).is_err()
        {
            let _ = destroy_frame(replacement);
            crate::log_warn!(target: "ui4/blueprint-frame"; "immutable refresh broker swap failed owner={:?} window={} old_frame={} replacement_frame={} action=retain-surflive-front\n", owner, window_id, previous.raw(), replacement.raw());
            return ERROR_UI4;
        }
        if damage.width == 0
            || damage.height == 0
            || publish_window_frame(owner, surface.window, damage).is_err()
        {
            if let Some((previous, replacement)) = immutable_replacement {
                let _ = replace_window_frame(owner, surface.window, previous);
                let _ = publish_window_frame(owner, surface.window, DamageRect::FULL);
                let _ = destroy_frame(replacement);
                crate::log_warn!(target: "ui4/blueprint-frame"; "immutable refresh publish failed owner={:?} window={} old_frame={} replacement_frame={} action=old-front-restored\n", owner, window_id, previous.raw(), replacement.raw());
            }
            return ERROR_UI4;
        }
        if let Some((previous, replacement)) = immutable_replacement {
            surface.frame = replacement;
            if let Err(error) = destroy_frame(previous) {
                if error == FramePoolError::Busy {
                    RETIRED_FRAMES.lock().push(previous);
                }
            }
            crate::log_info!(target: "ui4/blueprint-frame"; "immutable refresh committed owner={:?} window={} old_frame={} frame={} extent={}x{} action=broker-swap-after-complete-publish old_release=surflive\n", owner, window_id, previous.raw(), replacement.raw(), surface.width, surface.height);
        }
    }
    surface
        .retained_text_layers
        .truncate(surface.retained_text_cursor);
    surface.retained_text_cursor = 0;
    surface.retained_text_rendered = false;
    surface.stamped_text_layers.clear();
    surface.stamped_text_cursor = 0;
    surface.stamped_text_pending = None;
    surface.stamped_text_rendered = false;
    let launch_selection = surface.launch_selection.take();
    let launched_window = surface.window;
    drop(surfaces);
    if let Some((source, x, y)) = launch_selection {
        match super::input_broker::select_window_for_cursor_at(source, owner, launched_window, x, y)
        {
            Ok(_) => crate::log_info!(target: "ui4";
                "ui4/input: desktop shell selected owner={:?} window={} cursor={}:{}:{} position={},{} trigger=desktop-shell-launch\n",
                owner,
                launched_window.raw(),
                source.controller_id,
                source.slot_id,
                source.ep_target,
                x,
                y,
            ),
            Err(error) => crate::log_warn!(target: "ui4";
                "ui4/input: desktop shell selection failed owner={:?} window={} cursor={}:{}:{} error={:?}\n",
                owner,
                launched_window.raw(),
                source.controller_id,
                source.slot_id,
                source.ep_target,
                error,
            ),
        }
    }
    0
}

/// Publish the active BlueprintScene lease after its compute producer has
/// finished writing the exact UI4 RGBA8 surface. This is intentionally not a
/// RenderScene3d handoff: it requires UI4's compute release fence and the
/// active BlueprintScene write lease.
pub extern "C" fn trueos_cabi_ui4_scene_compute_frame_publish(
    window_id: u32,
    damage_x: u32,
    damage_y: u32,
    damage_width: u32,
    damage_height: u32,
) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let mut payload = [0u8; 8];
        payload[..4].copy_from_slice(&damage_width.to_le_bytes());
        payload[4..].copy_from_slice(&damage_height.to_le_bytes());
        return guest_status(
            trueos_vm::vmcall::OP_BP_UI4_SCENE_COMPUTE_FRAME_PUBLISH,
            window_id as u64,
            pack_u32_pair(damage_x, damage_y),
            &payload,
        );
    }
    let Some(owner) = blueprint_owner() else {
        return ERROR_CONTEXT;
    };
    {
        let mut surfaces = SURFACES.lock();
        let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
            return ERROR_NOT_FOUND;
        };
        if surface.write_lease.is_none() {
            return ERROR_STATE;
        }
        if surface.pending_gpu_release.is_none() || surface.pending_render_release.is_some() {
            return ERROR_STATE;
        }
    }
    trueos_cabi_ui4_solara_frame_publish(window_id, damage_x, damage_y, damage_width, damage_height)
}

pub extern "C" fn trueos_cabi_ui4_solara_frame_close(window_id: u32) -> i32 {
    trueos_cabi_ui4_solara_frame_close_requested(window_id, 0)
}

pub unsafe extern "C" fn trueos_cabi_image_source_info(
    name_ptr: *const u8,
    name_len: usize,
    out: *mut BlueprintImageSourceInfo,
) -> i32 {
    if name_ptr.is_null() || out.is_null() || name_len == 0 {
        return ERROR_INVALID;
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return unsafe { guest_image_source_info(name_ptr, name_len, out) };
    }
    let Ok(name) = core::str::from_utf8(unsafe { core::slice::from_raw_parts(name_ptr, name_len) })
    else {
        return ERROR_INVALID;
    };
    match blueprint_image_source_info(name) {
        Ok(info) => {
            unsafe { out.write(info) };
            0
        }
        Err(error) => error,
    }
}

pub unsafe extern "C" fn trueos_cabi_image_source_read(
    name_ptr: *const u8,
    name_len: usize,
    offset: usize,
    out_ptr: *mut u8,
    out_cap: usize,
) -> isize {
    if name_ptr.is_null() || out_ptr.is_null() || name_len == 0 || out_cap == 0 {
        return ERROR_INVALID as isize;
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return unsafe { guest_image_source_read(name_ptr, name_len, offset, out_ptr, out_cap) };
    }
    let Ok(name) = core::str::from_utf8(unsafe { core::slice::from_raw_parts(name_ptr, name_len) })
    else {
        return ERROR_INVALID as isize;
    };
    let out = unsafe { core::slice::from_raw_parts_mut(out_ptr, out_cap) };
    copy_blueprint_image_source(name, offset, out)
        .map(|copied| copied as isize)
        .unwrap_or_else(|error| error as isize)
}

pub extern "C" fn trueos_cabi_ui4_solara_frame_close_requested(window_id: u32, flags: u32) -> i32 {
    if flags & !CLOSE_VALID_FLAGS != 0 {
        return ERROR_INVALID;
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return guest_status(
            trueos_vm::vmcall::OP_BP_UI4_SOLARA_FRAME_CLOSE,
            window_id as u64,
            flags as u64,
            &[],
        );
    }
    reap_retired_frames();
    let Some(owner) = blueprint_owner() else {
        return ERROR_CONTEXT;
    };
    let surface = {
        let mut surfaces = SURFACES.lock();
        let Some(slot) = surfaces
            .iter()
            .position(|surface| surface.owner == owner && surface.window.raw() == window_id)
        else {
            return ERROR_NOT_FOUND;
        };
        if surfaces[slot].gpu_submission_unretired || surfaces[slot].vgpu_surface.is_some() {
            return ERROR_BUSY;
        }
        surfaces.remove(slot)
    };
    let release = if flags & CLOSE_PERSIST_FINAL_FRAME != 0 {
        BlueprintSurfaceRelease::AnimatedAndPersistFinalFrame
    } else {
        BlueprintSurfaceRelease::Animated
    };
    release_surface(surface, release);
    0
}

unsafe fn guest_font_sizes(out: *mut TrueosUi4SolaraFontSize, out_cap: usize) -> isize {
    let response_cap = out_cap.min(MAX_NATIVE_FONT_SIZES);
    let response_bytes =
        response_cap.saturating_mul(core::mem::size_of::<TrueosUi4SolaraFontSize>());
    let mut response = alloc::vec![0u8; response_bytes];
    let (status, data) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_UI4_SOLARA_FONT_SIZES,
        response_cap as u64,
        0,
        &[],
        response.as_mut_slice(),
    );
    if status != trueos_vm::vmcall::STATUS_OK {
        return ERROR_UI4 as isize;
    }
    let result = data as i64;
    if result < 0 {
        return result as isize;
    }
    let count = result as usize;
    let copied_entries = count.min(response_cap);
    let copied_bytes = copied_entries * core::mem::size_of::<TrueosUi4SolaraFontSize>();
    if copied_bytes != 0 {
        // SAFETY: the caller supplied capacity for response_cap entries and
        // call_with_payload initialized the copied response bytes.
        unsafe {
            core::ptr::copy_nonoverlapping(response.as_ptr(), out.cast::<u8>(), copied_bytes);
        }
    }
    count as isize
}

unsafe fn guest_image_source_info(
    name_ptr: *const u8,
    name_len: usize,
    out: *mut BlueprintImageSourceInfo,
) -> i32 {
    let name = unsafe { core::slice::from_raw_parts(name_ptr, name_len) };
    let mut response = [0u8; core::mem::size_of::<BlueprintImageSourceInfo>()];
    let (status, data) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_IMAGE_SOURCE_INFO,
        0,
        0,
        name,
        &mut response,
    );
    if status != trueos_vm::vmcall::STATUS_OK {
        return ERROR_UI4;
    }
    let result = data as i64 as i32;
    if result != 0 {
        return result;
    }
    unsafe { out.write(core::ptr::read_unaligned(response.as_ptr().cast())) };
    0
}

unsafe fn guest_image_source_read(
    name_ptr: *const u8,
    name_len: usize,
    offset: usize,
    out_ptr: *mut u8,
    out_cap: usize,
) -> isize {
    if out_cap > IMAGE_SOURCE_READ_CHUNK_BYTES {
        return ERROR_INVALID as isize;
    }
    let name = unsafe { core::slice::from_raw_parts(name_ptr, name_len) };
    let mut response = alloc::vec![0u8; out_cap];
    let (status, data) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_IMAGE_SOURCE_READ,
        offset as u64,
        out_cap as u64,
        name,
        response.as_mut_slice(),
    );
    if status != trueos_vm::vmcall::STATUS_OK {
        return ERROR_UI4 as isize;
    }
    let copied = data as usize;
    if copied > out_cap {
        return ERROR_UI4 as isize;
    }
    unsafe { core::ptr::copy_nonoverlapping(response.as_ptr(), out_ptr, copied) };
    copied as isize
}

unsafe fn guest_context_menu_register(
    window_id: u32,
    entries: &[TrueosUi4ContextMenuEntry],
) -> i32 {
    let mut payload = Vec::with_capacity(
        CONTEXT_MENU_WIRE_HEADER_BYTES.saturating_add(
            entries
                .len()
                .saturating_mul(CONTEXT_MENU_ENTRY_WIRE_HEADER_BYTES + 16),
        ),
    );
    payload.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    for entry in entries {
        if entry.label_ptr.is_null()
            || entry.label_len == 0
            || entry.label_len > MAX_CONTEXT_MENU_LABEL_BYTES
        {
            return ERROR_INVALID;
        }
        let Some(required) = payload
            .len()
            .checked_add(CONTEXT_MENU_ENTRY_WIRE_HEADER_BYTES)
            .and_then(|bytes| bytes.checked_add(entry.label_len))
        else {
            return ERROR_INVALID;
        };
        if required > trueos_vm::vmcall::PAYLOAD_CAP {
            return ERROR_INVALID;
        }
        // SAFETY: the C ABI requires `label_len` readable bytes per entry.
        let label = unsafe { core::slice::from_raw_parts(entry.label_ptr, entry.label_len) };
        payload.extend_from_slice(&entry.action_id.to_le_bytes());
        payload.extend_from_slice(&entry.enabled.to_le_bytes());
        payload.extend_from_slice(&(entry.label_len as u32).to_le_bytes());
        payload.extend_from_slice(label);
    }
    guest_status(
        trueos_vm::vmcall::OP_BP_UI4_CONTEXT_MENU_REGISTER,
        window_id as u64,
        0,
        payload.as_slice(),
    )
}

unsafe fn guest_context_menu_event_take(
    window_id: u32,
    out: *mut TrueosUi4ContextMenuEvent,
) -> i32 {
    let mut response = [0u8; core::mem::size_of::<TrueosUi4ContextMenuEvent>()];
    let (status, data) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_UI4_CONTEXT_MENU_EVENT_TAKE,
        window_id as u64,
        0,
        &[],
        &mut response,
    );
    if status != trueos_vm::vmcall::STATUS_OK {
        return ERROR_UI4;
    }
    let result = data as i64 as i32;
    if result != 0 {
        return result;
    }
    let event = unsafe { core::ptr::read_unaligned(response.as_ptr().cast()) };
    unsafe { out.write(event) };
    0
}

unsafe fn guest_pointer_event_take(window_id: u32, out: *mut TrueosUi4PointerEvent) -> i32 {
    let mut response = [0u8; core::mem::size_of::<TrueosUi4PointerEvent>()];
    let (status, data) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_UI4_SCENE_POINTER_EVENT_TAKE,
        window_id as u64,
        0,
        &[],
        &mut response,
    );
    if status != trueos_vm::vmcall::STATUS_OK {
        return ERROR_UI4;
    }
    let result = data as i64 as i32;
    if result != 0 {
        return result;
    }
    let event = unsafe { core::ptr::read_unaligned(response.as_ptr().cast()) };
    unsafe { out.write(event) };
    0
}

unsafe fn guest_keyboard_event_take(
    window_id: u32,
    out: *mut crate::r::keyboard::TrueosKeyboardOutputEvent,
) -> i32 {
    let mut response = [0u8; core::mem::size_of::<crate::r::keyboard::TrueosKeyboardOutputEvent>()];
    let (status, data) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_UI4_SCENE_KEYBOARD_EVENT_TAKE,
        window_id as u64,
        0,
        &[],
        &mut response,
    );
    if status != trueos_vm::vmcall::STATUS_OK {
        return ERROR_UI4;
    }
    let result = data as i64 as i32;
    if result != 0 {
        return result;
    }
    let event = unsafe { core::ptr::read_unaligned(response.as_ptr().cast()) };
    unsafe { out.write(event) };
    0
}

unsafe fn guest_pan_event_take(window_id: u32, out: *mut TrueosUi4PanEvent) -> i32 {
    let mut response = [0u8; core::mem::size_of::<TrueosUi4PanEvent>()];
    let (status, data) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_UI4_SCENE_PAN_EVENT_TAKE,
        window_id as u64,
        0,
        &[],
        &mut response,
    );
    if status != trueos_vm::vmcall::STATUS_OK {
        return ERROR_UI4;
    }
    let result = data as i64 as i32;
    if result != 0 {
        return result;
    }
    let event = unsafe { core::ptr::read_unaligned(response.as_ptr().cast()) };
    unsafe { out.write(event) };
    0
}

unsafe fn guest_resize_event_take(window_id: u32, out: *mut TrueosUi4ResizeEvent) -> i32 {
    let mut response = [0u8; core::mem::size_of::<TrueosUi4ResizeEvent>()];
    let (status, data) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_UI4_SCENE_RESIZE_EVENT_TAKE,
        window_id as u64,
        0,
        &[],
        &mut response,
    );
    if status != trueos_vm::vmcall::STATUS_OK {
        return ERROR_UI4;
    }
    let result = data as i64 as i32;
    if result != 0 {
        return result;
    }
    let event = unsafe { core::ptr::read_unaligned(response.as_ptr().cast()) };
    unsafe { out.write(event) };
    0
}

unsafe fn guest_keyboard_state(window_id: u32, out: *mut TrueosUi4KeyboardState) -> i32 {
    let mut response = [0u8; core::mem::size_of::<TrueosUi4KeyboardState>()];
    let (status, data) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_UI4_SCENE_KEYBOARD_STATE,
        window_id as u64,
        0,
        &[],
        &mut response,
    );
    if status != trueos_vm::vmcall::STATUS_OK {
        return ERROR_UI4;
    }
    let result = data as i64 as i32;
    if result != 0 {
        return result;
    }
    let state = unsafe { core::ptr::read_unaligned(response.as_ptr().cast()) };
    unsafe { out.write(state) };
    0
}

unsafe fn guest_input_routes(
    window_id: u32,
    out: *mut TrueosUi4InputRouteState,
    out_cap: u32,
) -> isize {
    let response_cap = (out_cap as usize).min(MAX_INPUT_ROUTES);
    let mut response =
        alloc::vec![0u8; response_cap * core::mem::size_of::<TrueosUi4InputRouteState>()];
    let (status, data) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_UI4_SCENE_INPUT_ROUTES,
        window_id as u64,
        response_cap as u64,
        &[],
        response.as_mut_slice(),
    );
    if status != trueos_vm::vmcall::STATUS_OK {
        return ERROR_UI4 as isize;
    }
    let result = data as i64;
    if result < 0 {
        return result as isize;
    }
    let copied = (result as usize).min(response_cap);
    if copied != 0 {
        // SAFETY: out_cap covers response_cap records and the vmcall copied
        // exactly the initialized response bytes for `copied` records.
        unsafe {
            core::ptr::copy_nonoverlapping(
                response.as_ptr(),
                out.cast::<u8>(),
                copied * core::mem::size_of::<TrueosUi4InputRouteState>(),
            );
        }
    }
    result as isize
}

unsafe fn guest_text_rows(
    window_id: u32,
    font_id: u32,
    native_scale: u32,
    dst_x: i32,
    dst_y: i32,
    rgba: u32,
    rows: *const TrueosUi4SolaraTextRow,
    row_count: usize,
) -> i32 {
    if rows.is_null() || row_count == 0 || row_count > MAX_TEXT_ROWS {
        return ERROR_INVALID;
    }
    // SAFETY: the Blueprint ABI promises row_count readable row descriptors.
    let input = unsafe { core::slice::from_raw_parts(rows, row_count) };
    let mut payload = Vec::with_capacity(
        TEXT_ROWS_WIRE_HEADER_BYTES
            .saturating_add(row_count.saturating_mul(TEXT_ROW_WIRE_HEADER_BYTES + 32)),
    );
    payload.extend_from_slice(&dst_x.to_le_bytes());
    payload.extend_from_slice(&dst_y.to_le_bytes());
    payload.extend_from_slice(&rgba.to_le_bytes());
    payload.extend_from_slice(&(row_count as u32).to_le_bytes());
    for row in input {
        if row.text_ptr.is_null() || row.text_len == 0 || row.text_len > MAX_TEXT_ROW_BYTES {
            return ERROR_INVALID;
        }
        let Some(required) = payload
            .len()
            .checked_add(TEXT_ROW_WIRE_HEADER_BYTES)
            .and_then(|bytes| bytes.checked_add(row.text_len))
        else {
            return ERROR_INVALID;
        };
        if required > trueos_vm::vmcall::PAYLOAD_CAP {
            return ERROR_INVALID;
        }
        // SAFETY: each ABI row promises text_len readable bytes.
        let text = unsafe { core::slice::from_raw_parts(row.text_ptr, row.text_len) };
        payload.extend_from_slice(&row.x.to_bits().to_le_bytes());
        payload.extend_from_slice(&row.y.to_bits().to_le_bytes());
        payload.extend_from_slice(&(row.text_len as u32).to_le_bytes());
        payload.extend_from_slice(text);
    }
    guest_status(
        trueos_vm::vmcall::OP_BP_UI4_SOLARA_TEXT_ROWS,
        window_id as u64,
        pack_u32_pair(font_id, native_scale),
        payload.as_slice(),
    )
}

unsafe fn guest_text_scene(
    window_id: u32,
    font_id: u32,
    viewport_width: u32,
    viewport_height: u32,
    rgba: u32,
    rows: *const TrueosUi4SolaraSceneTextRow,
    row_count: usize,
) -> i32 {
    if rows.is_null() || row_count == 0 || row_count > MAX_TEXT_ROWS {
        return ERROR_INVALID;
    }
    // SAFETY: the Blueprint ABI promises row_count readable row descriptors.
    let input = unsafe { core::slice::from_raw_parts(rows, row_count) };
    let mut payload = Vec::with_capacity(
        TEXT_SCENE_WIRE_HEADER_BYTES
            .saturating_add(row_count.saturating_mul(TEXT_SCENE_ROW_WIRE_HEADER_BYTES + 32)),
    );
    payload.extend_from_slice(&viewport_width.to_le_bytes());
    payload.extend_from_slice(&viewport_height.to_le_bytes());
    payload.extend_from_slice(&rgba.to_le_bytes());
    payload.extend_from_slice(&(row_count as u32).to_le_bytes());
    for row in input {
        if row.text_ptr.is_null() || row.text_len == 0 || row.text_len > MAX_TEXT_ROW_BYTES {
            return ERROR_INVALID;
        }
        let Some(required) = payload
            .len()
            .checked_add(TEXT_SCENE_ROW_WIRE_HEADER_BYTES)
            .and_then(|bytes| bytes.checked_add(row.text_len))
        else {
            return ERROR_INVALID;
        };
        if required > trueos_vm::vmcall::PAYLOAD_CAP {
            return ERROR_INVALID;
        }
        // SAFETY: each ABI row promises text_len readable bytes.
        let text = unsafe { core::slice::from_raw_parts(row.text_ptr, row.text_len) };
        payload.extend_from_slice(&row.x.to_bits().to_le_bytes());
        payload.extend_from_slice(&row.y.to_bits().to_le_bytes());
        payload.extend_from_slice(&row.font_pixels.to_bits().to_le_bytes());
        payload.extend_from_slice(&(row.text_len as u32).to_le_bytes());
        payload.extend_from_slice(text);
    }
    loop {
        let result = guest_status(
            trueos_vm::vmcall::OP_BP_UI4_SOLARA_TEXT_SCENE,
            window_id as u64,
            font_id as u64,
            payload.as_slice(),
        );
        if result != ERROR_BUSY {
            return result;
        }
        // The host has transferred owned strings to the Embassy FontKernel
        // service. Pace the synchronous ABI poll so a pending GPU ticket
        // cannot turn into a VM-exit and serial-log storm.
        trueos_vm::vmcall::sleep_ms(GUEST_TEXT_SCENE_BUSY_POLL_MS);
    }
}

unsafe fn guest_font_canvas(
    window_id: u32,
    font_id: u32,
    canvas_width: u32,
    canvas_height: u32,
    rows: *const TrueosUi4FontCanvasRow,
    row_count: usize,
) -> i32 {
    if rows.is_null() || row_count == 0 || row_count > MAX_FONT_CANVAS_ROWS {
        return ERROR_INVALID;
    }
    let input = unsafe { core::slice::from_raw_parts(rows, row_count) };
    let mut payload = Vec::with_capacity(
        FONT_CANVAS_WIRE_HEADER_BYTES
            .saturating_add(row_count.saturating_mul(FONT_CANVAS_ROW_WIRE_HEADER_BYTES + 32)),
    );
    payload.extend_from_slice(&canvas_width.to_le_bytes());
    payload.extend_from_slice(&canvas_height.to_le_bytes());
    payload.extend_from_slice(&(row_count as u32).to_le_bytes());
    for row in input {
        if row.text_ptr.is_null() || row.text_len == 0 || row.text_len > MAX_TEXT_ROW_BYTES {
            return ERROR_INVALID;
        }
        let Some(required) = payload
            .len()
            .checked_add(FONT_CANVAS_ROW_WIRE_HEADER_BYTES)
            .and_then(|bytes| bytes.checked_add(row.text_len))
        else {
            return ERROR_INVALID;
        };
        if required > trueos_vm::vmcall::PAYLOAD_CAP {
            return ERROR_INVALID;
        }
        let text = unsafe { core::slice::from_raw_parts(row.text_ptr, row.text_len) };
        payload.extend_from_slice(&row.x.to_bits().to_le_bytes());
        payload.extend_from_slice(&row.y.to_bits().to_le_bytes());
        payload.extend_from_slice(&row.font_pixels.to_bits().to_le_bytes());
        payload.extend_from_slice(&row.color_rgba.to_le_bytes());
        payload.extend_from_slice(&(row.text_len as u32).to_le_bytes());
        payload.extend_from_slice(text);
    }
    loop {
        let result = guest_status(
            trueos_vm::vmcall::OP_BP_UI4_FONT_CANVAS,
            window_id as u64,
            font_id as u64,
            payload.as_slice(),
        );
        if result != ERROR_BUSY {
            return result;
        }
        trueos_vm::vmcall::sleep_ms(GUEST_TEXT_SCENE_BUSY_POLL_MS);
    }
}

fn guest_status(op: u32, arg0: u64, arg1: u64, payload: &[u8]) -> i32 {
    let (status, data) = trueos_vm::vmcall::call_with_payload(op, arg0, arg1, payload, &mut []);
    if status == trueos_vm::vmcall::STATUS_OK {
        data as i64 as i32
    } else {
        ERROR_UI4
    }
}

fn expected_rgba8_len(width: u32, height: u32) -> Option<usize> {
    (width as usize)
        .checked_mul(height as usize)?
        .checked_mul(core::mem::size_of::<u32>())
}

pub(crate) fn begin_sprite_rgba8_upload(
    owner: WindowOwner,
    window_id: u32,
    sprite_id: u32,
    width: u32,
    height: u32,
) -> i32 {
    let Some(packed_len) = expected_rgba8_len(width, height) else {
        return ERROR_INVALID;
    };
    if sprite_id == 0 || sprite_id == TEXT_BACKBUFFER_SPRITE_ID || packed_len == 0 {
        return ERROR_INVALID;
    }
    let Some(row_bytes) = (width as usize).checked_mul(core::mem::size_of::<u32>()) else {
        return ERROR_INVALID;
    };
    let Some(pitch) =
        crate::intel::align_up(row_bytes, 64).and_then(|pitch| u32::try_from(pitch).ok())
    else {
        return ERROR_INVALID;
    };
    let Some(raw_bytes) = (pitch as usize).checked_mul(height as usize) else {
        return ERROR_INVALID;
    };
    let Some(bytes) = crate::intel::align_up(raw_bytes, crate::intel::WARM_ALIGN) else {
        return ERROR_INVALID;
    };
    if bytes > UI4_SCENE_SPRITE_MAX_BYTES.saturating_sub(UI4_SCENE_SOLID_SOURCE_BYTES) {
        return ERROR_INVALID;
    }

    let (gpu, retained_bytes, retained_sprites) = {
        let surfaces = SURFACES.lock();
        let Some(surface) = surfaces
            .iter()
            .find(|surface| surface.owner == owner && surface.window.raw() == window_id)
        else {
            return ERROR_NOT_FOUND;
        };
        if surface.gpu_submission_unretired {
            return ERROR_BUSY;
        }
        let Some(gpu) = allocate_sprite_gpu_va(surface, sprite_id, bytes) else {
            let retained_bytes = surface
                .sprites
                .iter()
                .fold(0usize, |total, (_, owned)| total.saturating_add(owned.bytes));
            crate::log_error!(target: "ui4/blueprint-frame";
                "sprite allocation failed owner={:?} window={} sprite={} extent={}x{} requested_bytes={} retained_sprites={} retained_bytes={} stage=ppgtt-va arena_bytes={}\n",
                owner,
                window_id,
                sprite_id,
                width,
                height,
                bytes,
                surface.sprites.len(),
                retained_bytes,
                UI4_SCENE_SPRITE_MAX_BYTES,
            );
            return ERROR_UI4;
        };
        let retained_bytes = surface
            .sprites
            .iter()
            .fold(0usize, |total, (_, owned)| total.saturating_add(owned.bytes));
        (gpu, retained_bytes, surface.sprites.len())
    };
    let Some((phys, virt)) = crate::dma::alloc_ppgtt(bytes, crate::intel::WARM_ALIGN) else {
        let pmm = crate::phys::pmm_stats();
        crate::log_error!(target: "ui4/blueprint-frame";
            "sprite allocation failed owner={:?} window={} sprite={} extent={}x{} requested_bytes={} retained_sprites={} retained_bytes={} stage=system-memory address_scope=all-pmm pmm_free_bytes={} pmm_largest_free_region={} pmm_free_regions={}\n",
            owner,
            window_id,
            sprite_id,
            width,
            height,
            bytes,
            retained_sprites,
            retained_bytes,
            pmm.map_or(0, |stats| stats.free_bytes),
            pmm.map_or(0, |stats| stats.largest_free_region),
            pmm.map_or(0, |stats| stats.free_regions),
        );
        return ERROR_UI4;
    };
    unsafe { core::ptr::write_bytes(virt, 0, bytes) };
    let Some(gpu_surface) = GpgpuRgba8Surface::new(phys, gpu, bytes, width, height, pitch) else {
        crate::dma::dealloc(virt, bytes);
        return ERROR_UI4;
    };
    let upload = Rgba8Upload {
        sprite_id,
        owned: OwnedRgba8Surface {
            surface: gpu_surface,
            virt,
            bytes,
        },
        packed_len,
        written: 0,
    };
    let old = {
        let mut surfaces = SURFACES.lock();
        let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
            drop(surfaces);
            destroy_rgba8_surface(upload.owned);
            return ERROR_NOT_FOUND;
        };
        surface.sprite_upload.replace(upload)
    };
    if let Some(old) = old {
        destroy_rgba8_surface(old.owned);
    }
    0
}

fn allocate_sprite_gpu_va(
    surface: &BlueprintSceneSurface,
    replacing_sprite_id: u32,
    bytes: usize,
) -> Option<u64> {
    let base = UI4_SCENE_SPRITE_GPU.checked_add(UI4_SCENE_SOLID_SOURCE_BYTES as u64)?;
    let limit = UI4_SCENE_SPRITE_GPU.checked_add(UI4_SCENE_SPRITE_MAX_BYTES as u64)?;
    let mut spans = surface
        .sprites
        .iter()
        .filter(|(sprite_id, _)| *sprite_id != replacing_sprite_id)
        .map(|(_, owned)| (owned.surface.gpu, owned.surface.gpu.saturating_add(owned.bytes as u64)))
        .collect::<Vec<_>>();
    spans.sort_unstable_by_key(|span| span.0);
    let mut candidate = base;
    for (start, end) in spans {
        let candidate_end = candidate.checked_add(bytes as u64)?;
        if candidate_end <= start {
            return Some(candidate);
        }
        candidate = candidate.max(end);
    }
    (candidate.checked_add(bytes as u64)? <= limit).then_some(candidate)
}

pub(crate) fn write_sprite_rgba8_upload_chunk(
    owner: WindowOwner,
    window_id: u32,
    sprite_id: u32,
    offset: usize,
    bytes: &[u8],
) -> i32 {
    if bytes.is_empty() || offset & 3 != 0 || bytes.len() & 3 != 0 {
        return ERROR_INVALID;
    }
    let mut surfaces = SURFACES.lock();
    let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
        return ERROR_NOT_FOUND;
    };
    let Some(upload) = surface.sprite_upload.as_mut() else {
        return ERROR_STATE;
    };
    if upload.sprite_id != sprite_id || offset != upload.written {
        return ERROR_INVALID;
    }
    let Some(end) = offset.checked_add(bytes.len()) else {
        return ERROR_INVALID;
    };
    if end > upload.packed_len {
        return ERROR_INVALID;
    }
    let row_bytes = upload.owned.surface.width as usize * core::mem::size_of::<u32>();
    let pitch = upload.owned.surface.pitch_bytes as usize;
    let mut source_offset = 0usize;
    let mut packed_offset = offset;
    while source_offset < bytes.len() {
        let row = packed_offset / row_bytes;
        let column = packed_offset % row_bytes;
        let count = core::cmp::min(row_bytes - column, bytes.len() - source_offset);
        let destination_offset = row * pitch + column;
        unsafe {
            core::ptr::copy_nonoverlapping(
                bytes.as_ptr().add(source_offset),
                upload.owned.virt.add(destination_offset),
                count,
            );
        }
        source_offset += count;
        packed_offset += count;
    }
    upload.written = end;
    0
}

pub(crate) fn finish_sprite_rgba8_upload(
    owner: WindowOwner,
    window_id: u32,
    sprite_id: u32,
) -> i32 {
    let upload = {
        let mut surfaces = SURFACES.lock();
        let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
            return ERROR_NOT_FOUND;
        };
        surface.sprite_upload.take()
    };
    let Some(upload) = upload else {
        return ERROR_STATE;
    };
    if upload.sprite_id != sprite_id || upload.written != upload.packed_len {
        destroy_rgba8_surface(upload.owned);
        return ERROR_INVALID;
    }
    crate::intel::dma_flush(upload.owned.virt, upload.owned.bytes);
    let old = {
        let mut surfaces = SURFACES.lock();
        let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
            drop(surfaces);
            destroy_rgba8_surface(upload.owned);
            return ERROR_NOT_FOUND;
        };
        if let Some(slot) = surface
            .sprites
            .iter()
            .position(|(current_id, _)| *current_id == sprite_id)
        {
            Some(core::mem::replace(&mut surface.sprites[slot].1, upload.owned))
        } else {
            surface.sprites.push((sprite_id, upload.owned));
            None
        }
    };
    if let Some(old) = old {
        destroy_rgba8_surface(old);
    }
    crate::log_trace!(target: "ui4/blueprint-frame";
        "sprite source ready owner={:?} window={} sprite={} extent={}x{} bytes={} phys=0x{:X} gpu=0x{:X} backing=all-pmm-contiguous+ppgtt\n",
        owner,
        window_id,
        sprite_id,
        upload.owned.surface.width,
        upload.owned.surface.height,
        upload.owned.bytes,
        upload.owned.surface.phys,
        upload.owned.surface.gpu,
    );
    0
}

pub(crate) fn begin_sprite_scene(owner: WindowOwner, window_id: u32, expected: usize) -> i32 {
    if expected > MAX_SPRITE_QUADS {
        return ERROR_INVALID;
    }
    let mut surfaces = SURFACES.lock();
    let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
        return ERROR_NOT_FOUND;
    };
    if surface.write_lease.is_none() || surface.sprite_clear_rgba.is_none() {
        return ERROR_STATE;
    }
    if surface.gpu_submission_unretired {
        return ERROR_BUSY;
    }
    surface.sprite_scene_upload = Some(SpriteSceneUpload {
        expected,
        quads: Vec::with_capacity(expected),
    });
    0
}

pub(crate) fn append_sprite_scene(
    owner: WindowOwner,
    window_id: u32,
    offset: usize,
    quads: &[TrueosUi4SpriteQuad],
) -> i32 {
    let mut surfaces = SURFACES.lock();
    let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
        return ERROR_NOT_FOUND;
    };
    let Some(upload) = surface.sprite_scene_upload.as_mut() else {
        return ERROR_STATE;
    };
    if offset != upload.quads.len()
        || offset.saturating_add(quads.len()) > upload.expected
        || quads.iter().any(|quad| !valid_sprite_quad(*quad))
    {
        return ERROR_INVALID;
    }
    upload.quads.extend_from_slice(quads);
    0
}

pub(crate) fn append_sprite_scene_bytes(
    owner: WindowOwner,
    window_id: u32,
    offset: usize,
    bytes: &[u8],
) -> i32 {
    let record_bytes = core::mem::size_of::<TrueosUi4SpriteQuad>();
    if bytes.is_empty() || !bytes.len().is_multiple_of(record_bytes) {
        return ERROR_INVALID;
    }
    let mut quads = Vec::with_capacity(bytes.len() / record_bytes);
    for record in bytes.chunks_exact(record_bytes) {
        let mut words = [0u32; 19];
        for (word, raw) in words.iter_mut().zip(record.chunks_exact(4)) {
            *word = u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]);
        }
        quads.push(sprite_quad_from_words(words));
    }
    append_sprite_scene(owner, window_id, offset, &quads)
}

fn sprite_quad_from_words(words: [u32; 19]) -> TrueosUi4SpriteQuad {
    TrueosUi4SpriteQuad {
        sprite_id: words[0],
        c0_x: f32::from_bits(words[1]),
        c0_y: f32::from_bits(words[2]),
        c0_u: f32::from_bits(words[3]),
        c0_v: f32::from_bits(words[4]),
        c1_x: f32::from_bits(words[5]),
        c1_y: f32::from_bits(words[6]),
        c1_u: f32::from_bits(words[7]),
        c1_v: f32::from_bits(words[8]),
        c2_x: f32::from_bits(words[9]),
        c2_y: f32::from_bits(words[10]),
        c2_u: f32::from_bits(words[11]),
        c2_v: f32::from_bits(words[12]),
        c3_x: f32::from_bits(words[13]),
        c3_y: f32::from_bits(words[14]),
        c3_u: f32::from_bits(words[15]),
        c3_v: f32::from_bits(words[16]),
        color_rgba: words[17],
        flags: words[18],
    }
}

fn valid_sprite_quad(quad: TrueosUi4SpriteQuad) -> bool {
    let values = [
        quad.c0_x, quad.c0_y, quad.c0_u, quad.c0_v, quad.c1_x, quad.c1_y, quad.c1_u, quad.c1_v,
        quad.c2_x, quad.c2_y, quad.c2_u, quad.c2_v, quad.c3_x, quad.c3_y, quad.c3_u, quad.c3_v,
    ];
    values.iter().all(|value| value.is_finite()) && quad.flags & !SPRITE_QUAD_VALID_FLAGS == 0
}

#[derive(Copy, Clone)]
enum AlphaRectConversion {
    Exact(GpgpuAlphaBlendWorklistDesc),
    Clipped,
    Unsupported,
}

fn rounded_sprite_coordinate(value: f32) -> Option<i32> {
    const EPSILON: f32 = 0.01;

    if !value.is_finite() {
        return None;
    }
    let rounded = libm::roundf(value);
    if (value - rounded).abs() > EPSILON
        || f64::from(rounded) < f64::from(i32::MIN)
        || f64::from(rounded) > f64::from(i32::MAX)
    {
        return None;
    }
    Some(rounded as i32)
}

fn gpgpu_sprite_quad_descriptor(
    quad: TrueosUi4SpriteQuad,
    premultiplied_source: bool,
) -> GpgpuSpriteQuadWorklistDesc {
    GpgpuSpriteQuadWorklistDesc {
        c0_x: quad.c0_x,
        c0_y: quad.c0_y,
        c0_u: quad.c0_u,
        c0_v: quad.c0_v,
        c1_x: quad.c1_x,
        c1_y: quad.c1_y,
        c1_u: quad.c1_u,
        c1_v: quad.c1_v,
        c2_x: quad.c2_x,
        c2_y: quad.c2_y,
        c2_u: quad.c2_u,
        c2_v: quad.c2_v,
        c3_x: quad.c3_x,
        c3_y: quad.c3_y,
        c3_u: quad.c3_u,
        c3_v: quad.c3_v,
        color_rgba: quad.color_rgba,
        flags: if quad.flags & SPRITE_QUAD_FLAG_SRC_OVER != 0 {
            SPRITE_QUAD_WORKLIST_FLAG_SRC_OVER
        } else {
            0
        } | if premultiplied_source {
            SPRITE_QUAD_WORKLIST_FLAG_PREMUL_SRC
        } else {
            0
        },
    }
}

/// Convert an axis-aligned, pixel-locked sprite to the compact alpha worklist.
/// Anything involving resampling, rotation, flipping, or sampler clamping stays
/// on the arbitrary-quad compute path so the native scene contract is exact.
fn alpha_rect_descriptor(
    quad: TrueosUi4SpriteQuad,
    source: GpgpuRgba8Surface,
    destination: GpgpuRgba8Surface,
) -> AlphaRectConversion {
    let geometry = [
        rounded_sprite_coordinate(quad.c0_x),
        rounded_sprite_coordinate(quad.c0_y),
        rounded_sprite_coordinate(quad.c1_x),
        rounded_sprite_coordinate(quad.c1_y),
        rounded_sprite_coordinate(quad.c2_x),
        rounded_sprite_coordinate(quad.c2_y),
        rounded_sprite_coordinate(quad.c3_x),
        rounded_sprite_coordinate(quad.c3_y),
    ];
    let [
        Some(c0_x),
        Some(c0_y),
        Some(c1_x),
        Some(c1_y),
        Some(c2_x),
        Some(c2_y),
        Some(c3_x),
        Some(c3_y),
    ] = geometry
    else {
        return AlphaRectConversion::Unsupported;
    };

    let source_pixels = [
        rounded_sprite_coordinate(quad.c0_u * source.width as f32),
        rounded_sprite_coordinate(quad.c0_v * source.height as f32),
        rounded_sprite_coordinate(quad.c1_u * source.width as f32),
        rounded_sprite_coordinate(quad.c1_v * source.height as f32),
        rounded_sprite_coordinate(quad.c2_u * source.width as f32),
        rounded_sprite_coordinate(quad.c2_v * source.height as f32),
        rounded_sprite_coordinate(quad.c3_u * source.width as f32),
        rounded_sprite_coordinate(quad.c3_v * source.height as f32),
    ];
    let [
        Some(s0_x),
        Some(s0_y),
        Some(s1_x),
        Some(s1_y),
        Some(s2_x),
        Some(s2_y),
        Some(s3_x),
        Some(s3_y),
    ] = source_pixels
    else {
        return AlphaRectConversion::Unsupported;
    };

    if c0_y != c1_y
        || c1_x != c2_x
        || c2_y != c3_y
        || c3_x != c0_x
        || s0_y != s1_y
        || s1_x != s2_x
        || s2_y != s3_y
        || s3_x != s0_x
    {
        return AlphaRectConversion::Unsupported;
    }

    let mut dst_x = i64::from(c0_x);
    let mut dst_y = i64::from(c0_y);
    let mut src_x = i64::from(s0_x);
    let mut src_y = i64::from(s0_y);
    let mut width = i64::from(c1_x) - dst_x;
    let mut height = i64::from(c3_y) - dst_y;
    let source_width = i64::from(s1_x) - src_x;
    let source_height = i64::from(s3_y) - src_y;
    if width <= 0 || height <= 0 || width != source_width || height != source_height {
        return AlphaRectConversion::Unsupported;
    }
    if src_x < 0
        || src_y < 0
        || src_x.saturating_add(width) > i64::from(source.width)
        || src_y.saturating_add(height) > i64::from(source.height)
    {
        return AlphaRectConversion::Unsupported;
    }

    let dst_width = i64::from(destination.width);
    let dst_height = i64::from(destination.height);
    if dst_x >= dst_width
        || dst_y >= dst_height
        || dst_x.saturating_add(width) <= 0
        || dst_y.saturating_add(height) <= 0
    {
        return AlphaRectConversion::Clipped;
    }
    if dst_x < 0 {
        let clipped = -dst_x;
        dst_x = 0;
        src_x += clipped;
        width -= clipped;
    }
    if dst_y < 0 {
        let clipped = -dst_y;
        dst_y = 0;
        src_y += clipped;
        height -= clipped;
    }
    width = width.min(dst_width - dst_x);
    height = height.min(dst_height - dst_y);
    if width <= 0 || height <= 0 {
        return AlphaRectConversion::Clipped;
    }

    if src_x > i64::from(u16::MAX)
        || src_y > i64::from(u16::MAX)
        || dst_x > i64::from(i16::MAX)
        || dst_y > i64::from(i16::MAX)
        || width > i64::from(u16::MAX)
        || height > i64::from(u16::MAX)
    {
        return AlphaRectConversion::Unsupported;
    }

    let [r, g, b, a] = quad.color_rgba.to_le_bytes();
    let mut flags = if quad.flags & SPRITE_QUAD_FLAG_SRC_OVER != 0 {
        ALPHA_BLEND_WORKLIST_FLAG_SRC_OVER
    } else {
        ALPHA_BLEND_WORKLIST_FLAG_COPY
    };
    if r != u8::MAX || g != u8::MAX || b != u8::MAX {
        flags |= ALPHA_BLEND_WORKLIST_FLAG_TINT_RGB;
    }
    if a != u8::MAX {
        flags |= ALPHA_BLEND_WORKLIST_FLAG_TINT_ALPHA;
    }

    AlphaRectConversion::Exact(GpgpuAlphaBlendWorklistDesc {
        src_xy: (src_x as u32) | ((src_y as u32) << 16),
        dst_xy: (dst_x as u32) | ((dst_y as u32) << 16),
        size: (width as u32) | ((height as u32) << 16),
        flags,
        color_rgba: quad.color_rgba,
    })
}

fn retained_font_canvas_surface(surface: &BlueprintSceneSurface) -> Option<GpgpuRgba8Surface> {
    surface
        .font_canvas
        .as_ref()
        .and_then(|canvas| canvas.ready.as_ref())
        .map(FontStampedBuffer::surface)
        .or_else(|| {
            surface
                .retained_text_backbuffer
                .as_ref()
                .map(GpgpuOwnedRgba8Surface::surface)
        })
}

pub(crate) fn finish_sprite_scene(owner: WindowOwner, window_id: u32) -> i32 {
    struct OwnedRun {
        sprite_id: u32,
        source: GpgpuRgba8Surface,
        descriptors: Vec<GpgpuSpriteQuadWorklistDesc>,
    }

    #[derive(Copy, Clone)]
    enum PreparedOp {
        Alpha {
            sprite_id: u32,
            source: GpgpuRgba8Surface,
            descriptor: GpgpuAlphaBlendWorklistDesc,
        },
        Quad {
            sprite_id: u32,
            source: GpgpuRgba8Surface,
            descriptor: GpgpuSpriteQuadWorklistDesc,
        },
    }

    enum PreparedBatch {
        Alpha {
            source: GpgpuRgba8Surface,
            descriptors: Vec<GpgpuAlphaBlendWorklistDesc>,
        },
        Quad {
            groups: Vec<OwnedRun>,
        },
    }

    impl PreparedBatch {
        fn descriptor_count(&self) -> usize {
            match self {
                Self::Alpha { descriptors, .. } => descriptors.len(),
                Self::Quad { groups } => groups
                    .iter()
                    .fold(0usize, |total, group| total.saturating_add(group.descriptors.len())),
            }
        }

        fn backend(&self) -> &'static str {
            match self {
                Self::Alpha { .. } => "gpgpu-alpha-rect-worklist",
                Self::Quad { .. } => "gpgpu-arbitrary-quad-fallback",
            }
        }
    }

    let backbuffer = render_retained_text_backbuffer_for_surface(owner, window_id);
    if backbuffer != 0 {
        return backbuffer;
    }

    let mut surfaces = SURFACES.lock();
    let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
        return ERROR_NOT_FOUND;
    };
    let Some(upload) = surface.sprite_scene_upload.take() else {
        return ERROR_STATE;
    };
    if upload.quads.len() != upload.expected {
        cancel_blueprint_sprite_frame_without_live_gpu(surface);
        return ERROR_INVALID;
    }
    let Some(lease) = surface.write_lease else {
        cancel_blueprint_sprite_frame_without_live_gpu(surface);
        return ERROR_STATE;
    };
    let Some(clear_rgba) = surface.sprite_clear_rgba else {
        cancel_blueprint_sprite_frame_without_live_gpu(surface);
        return ERROR_STATE;
    };
    if surface.gpu_submission_unretired {
        return ERROR_BUSY;
    }
    let solid = match ensure_solid_source(surface) {
        Ok(source) => source,
        Err(code) => {
            cancel_blueprint_sprite_frame_without_live_gpu(surface);
            return code;
        }
    };
    let Ok(destination) = gpgpu_rgba_surface(lease) else {
        cancel_blueprint_sprite_frame_without_live_gpu(surface);
        return ERROR_UI4;
    };

    let full_frame_copy = match upload.quads.as_slice() {
        [quad] if quad.sprite_id == TEXT_BACKBUFFER_SPRITE_ID => {
            let Some(source) = retained_font_canvas_surface(surface) else {
                cancel_blueprint_sprite_frame_without_live_gpu(surface);
                return ERROR_NOT_FOUND;
            };
            matches!(
                alpha_rect_descriptor(*quad, source, destination),
                AlphaRectConversion::Exact(descriptor)
                    if descriptor.dst_xy == 0
                        && descriptor.size
                            == surface.width | (surface.height << 16)
                        && descriptor.flags == ALPHA_BLEND_WORKLIST_FLAG_COPY
            )
        }
        _ => false,
    };
    let clear = TrueosUi4SpriteQuad {
        sprite_id: 0,
        c0_x: 0.0,
        c0_y: 0.0,
        c1_x: surface.width as f32,
        c1_y: 0.0,
        c2_x: surface.width as f32,
        c2_y: surface.height as f32,
        c3_x: 0.0,
        c3_y: surface.height as f32,
        color_rgba: clear_rgba,
        ..TrueosUi4SpriteQuad::default()
    };
    let mut prepared = Vec::with_capacity(upload.quads.len().saturating_add(1));
    if !full_frame_copy {
        prepared.push(PreparedOp::Quad {
            sprite_id: 0,
            source: solid.surface,
            descriptor: gpgpu_sprite_quad_descriptor(clear, false),
        });
    }
    for quad in upload.quads {
        let source = if quad.sprite_id == 0 {
            solid.surface
        } else if quad.sprite_id == TEXT_BACKBUFFER_SPRITE_ID {
            let Some(source) = retained_font_canvas_surface(surface) else {
                cancel_blueprint_sprite_frame_without_live_gpu(surface);
                return ERROR_NOT_FOUND;
            };
            source
        } else {
            let Some((_, source)) = surface
                .sprites
                .iter()
                .find(|(sprite_id, _)| *sprite_id == quad.sprite_id)
            else {
                cancel_blueprint_sprite_frame_without_live_gpu(surface);
                return ERROR_NOT_FOUND;
            };
            source.surface
        };
        // Physical XeLP has proven the general sprite-quad source-over path,
        // while the older compact alpha-rectangle source-over kernel can
        // accept a submission without retiring its marker. Keep opaque 1:1
        // copies eligible for the compact path, but route every blended sprite
        // through the newer ordered quad worklist.
        let conversion = if quad.sprite_id == 0 || quad.flags & SPRITE_QUAD_FLAG_SRC_OVER != 0 {
            AlphaRectConversion::Unsupported
        } else {
            alpha_rect_descriptor(quad, source, destination)
        };
        match conversion {
            AlphaRectConversion::Exact(descriptor) => prepared.push(PreparedOp::Alpha {
                sprite_id: quad.sprite_id,
                source,
                descriptor,
            }),
            AlphaRectConversion::Clipped => {}
            AlphaRectConversion::Unsupported => prepared.push(PreparedOp::Quad {
                sprite_id: quad.sprite_id,
                source,
                descriptor: gpgpu_sprite_quad_descriptor(
                    quad,
                    quad.sprite_id == TEXT_BACKBUFFER_SPRITE_ID,
                ),
            }),
        }
    }

    let batch_capacity = alpha_blend_worklist_max_descs().min(sprite_quad_worklist_max_descs());
    let mut batches = Vec::<PreparedBatch>::new();
    let mut cursor = 0usize;
    while cursor < prepared.len() {
        match prepared[cursor] {
            PreparedOp::Alpha {
                sprite_id, source, ..
            } => {
                let mut descriptors = Vec::new();
                while cursor < prepared.len() && descriptors.len() < batch_capacity {
                    let PreparedOp::Alpha {
                        sprite_id: next_sprite_id,
                        source: next_source,
                        descriptor,
                    } = prepared[cursor]
                    else {
                        break;
                    };
                    if next_sprite_id != sprite_id
                        || next_source.gpu != source.gpu
                        || next_source.phys != source.phys
                    {
                        break;
                    }
                    descriptors.push(descriptor);
                    cursor = cursor.saturating_add(1);
                }
                batches.push(PreparedBatch::Alpha {
                    source,
                    descriptors,
                });
            }
            PreparedOp::Quad { .. } => {
                let mut groups = Vec::<OwnedRun>::new();
                let mut descriptor_count = 0usize;
                while cursor < prepared.len() && descriptor_count < batch_capacity {
                    let PreparedOp::Quad {
                        sprite_id,
                        source,
                        descriptor,
                    } = prepared[cursor]
                    else {
                        break;
                    };
                    if let Some(group) = groups.last_mut()
                        && group.sprite_id == sprite_id
                    {
                        group.descriptors.push(descriptor);
                    } else {
                        groups.push(OwnedRun {
                            sprite_id,
                            source,
                            descriptors: alloc::vec![descriptor],
                        });
                    }
                    descriptor_count = descriptor_count.saturating_add(1);
                    cursor = cursor.saturating_add(1);
                }
                batches.push(PreparedBatch::Quad { groups });
            }
        }
    }

    let batch_count = batches.len();
    let mut final_release = None;
    for (batch_index, batch) in batches.iter().enumerate() {
        let descriptor_count = batch.descriptor_count();
        let backend = batch.backend();
        let queued = match batch {
            PreparedBatch::Alpha {
                source,
                descriptors,
            } => queue_blueprint_alpha_batch(*source, destination, descriptors),
            PreparedBatch::Quad { groups } => {
                let runs = groups
                    .iter()
                    .map(|group| GpgpuSpriteQuadWorklistRun {
                        src: group.source,
                        descs: &group.descriptors,
                    })
                    .collect::<Vec<_>>();
                queue_blueprint_sprite_batch(destination, &runs)
            }
        };
        let submission = match queued {
            Ok(submission) => submission,
            Err(Ui4CompositorSubmitError::Busy)
            | Err(Ui4CompositorSubmitError::SubmissionRejected) => {
                // No hardware accepted this batch. Discard the write lease so
                // the one-call Blueprint ABI can report a skipped frame and
                // acquire normally on the next tick. Retaining this upload
                // required a finish-only retry operation which the ABI does
                // not expose and left clients wedged in an active frame.
                cancel_blueprint_sprite_frame_without_live_gpu(surface);
                crate::log_warn!(target: "ui4/blueprint-frame";
                    "sprite scene skipped owner={:?} window={} frame={} buffer={} batch={}/{} descriptors={} backend={} reason=ui4-compositor-admission-timeout hardware_accepted=0 action=cancel-unsubmitted-frame+continue\n",
                    owner,
                    window_id,
                    lease.frame.raw(),
                    lease.buffer_index,
                    batch_index.saturating_add(1),
                    batch_count,
                    descriptor_count,
                    backend,
                );
                return ERROR_BUSY;
            }
            Err(Ui4CompositorSubmitError::Unavailable) => {
                // Admission rejected this batch before hardware ownership
                // changed. Release the producer lease so an interactive
                // client can begin a fresh frame on its next presentation.
                cancel_blueprint_sprite_frame_without_live_gpu(surface);
                crate::log_warn!(target: "ui4/blueprint-frame";
                    "sprite scene rejected owner={:?} window={} batch={}/{} backend={} reason=ui4-compositor-unavailable hardware_accepted=0 action=cancel-unsubmitted-frame+retry-next-presentation\n",
                    owner,
                    window_id,
                    batch_index.saturating_add(1),
                    batch_count,
                    backend,
                );
                return ERROR_UI4;
            }
            Err(Ui4CompositorSubmitError::InvalidWorklist) => {
                cancel_blueprint_sprite_frame_without_live_gpu(surface);
                crate::log_error!(target: "ui4/blueprint-frame";
                    "sprite scene rejected owner={:?} window={} batch={}/{} descriptors={} backend={} reason=invalid-private-ui4-worklist hardware_accepted=0 action=cancel-unsubmitted-frame\n",
                    owner,
                    window_id,
                    batch_index.saturating_add(1),
                    batch_count,
                    descriptor_count,
                    backend,
                );
                return ERROR_UI4;
            }
        };

        let final_batch = batch_index.saturating_add(1) == batch_count;
        let retire_started = crate::chronos::monotonic_nanos();
        loop {
            let completed = if final_batch {
                match poll_ui4_blueprint_sprite_scene(submission, destination) {
                    Ui4SpriteSceneCompletion::Pending => false,
                    Ui4SpriteSceneCompletion::Complete { stats, release } => {
                        if stats.descs != descriptor_count || stats.submits != 1 {
                            cancel_blueprint_sprite_frame_without_live_gpu(surface);
                            crate::log_error!(target: "ui4/blueprint-frame";
                                "sprite scene retirement contract mismatch owner={:?} window={} batch={}/{} expected_descs={} retired_descs={} retired_submits={} action=cancel-retired-frame\n",
                                owner,
                                window_id,
                                batch_index.saturating_add(1),
                                batch_count,
                                descriptor_count,
                                stats.descs,
                                stats.submits,
                            );
                            return ERROR_UI4;
                        }
                        final_release = Some(release);
                        true
                    }
                    Ui4SpriteSceneCompletion::Failed => {
                        quarantine_blueprint_sprite_submission(
                            surface,
                            owner,
                            window_id,
                            lease,
                            batch_index,
                            batch_count,
                            "ui4-compositor-retirement-failed",
                        );
                        return ERROR_UI4;
                    }
                }
            } else {
                match poll_ui4_compositor_submission(submission) {
                    Ui4CompositorCompletion::Pending => false,
                    Ui4CompositorCompletion::Complete(stats) => {
                        if stats.descs != descriptor_count || stats.submits != 1 {
                            cancel_blueprint_sprite_frame_without_live_gpu(surface);
                            crate::log_error!(target: "ui4/blueprint-frame";
                                "sprite scene retirement contract mismatch owner={:?} window={} batch={}/{} expected_descs={} retired_descs={} retired_submits={} action=cancel-retired-frame\n",
                                owner,
                                window_id,
                                batch_index.saturating_add(1),
                                batch_count,
                                descriptor_count,
                                stats.descs,
                                stats.submits,
                            );
                            return ERROR_UI4;
                        }
                        true
                    }
                    Ui4CompositorCompletion::Failed
                    | Ui4CompositorCompletion::InvalidSubmission => {
                        quarantine_blueprint_sprite_submission(
                            surface,
                            owner,
                            window_id,
                            lease,
                            batch_index,
                            batch_count,
                            "ui4-compositor-retirement-failed",
                        );
                        return ERROR_UI4;
                    }
                }
            };
            if completed {
                break;
            }
            if crate::chronos::monotonic_nanos().saturating_sub(retire_started)
                >= UI4_SPRITE_BATCH_TIMEOUT_NS
            {
                quarantine_blueprint_sprite_submission(
                    surface,
                    owner,
                    window_id,
                    lease,
                    batch_index,
                    batch_count,
                    "accepted-submission-not-retired",
                );
                return ERROR_UI4;
            }
            core::hint::spin_loop();
        }
    }
    let Some(final_release) = final_release else {
        cancel_blueprint_sprite_frame_without_live_gpu(surface);
        return ERROR_UI4;
    };
    surface.pending_gpu_release = Some(final_release);
    surface.sprite_clear_rgba = None;
    0
}

/// Cancel a sprite frame when no GPU submission can still own its target.
/// This covers pre-admission rejection and completed batches whose retirement
/// receipt was invalid. Uncertain accepted work must use quarantine instead.
fn cancel_blueprint_sprite_frame_without_live_gpu(surface: &mut BlueprintSceneSurface) {
    surface.sprite_scene_upload = None;
    surface.sprite_clear_rgba = None;
    surface.pending_gpu_release = None;
    surface.pending_render_release = None;
    let Some(cancelled) = surface.write_lease.take() else {
        return;
    };
    let replacement = (cancelled.frame != surface.frame).then_some(cancelled.frame);
    let _ = cancel_frame_buffer(cancelled);
    if let Some(replacement) = replacement
        && let Err(error) = destroy_frame(replacement)
        && error == FramePoolError::Busy
    {
        RETIRED_FRAMES.lock().push(replacement);
    }
}

fn queue_blueprint_alpha_batch(
    source: GpgpuRgba8Surface,
    destination: GpgpuRgba8Surface,
    descriptors: &[GpgpuAlphaBlendWorklistDesc],
) -> Result<Ui4CompositorSubmission, Ui4CompositorSubmitError> {
    let started = crate::chronos::monotonic_nanos();
    loop {
        match queue_ui4_blueprint_alpha_rects(source, destination, descriptors) {
            Err(Ui4CompositorSubmitError::Busy)
            | Err(Ui4CompositorSubmitError::SubmissionRejected)
                if crate::chronos::monotonic_nanos().saturating_sub(started)
                    < UI4_SPRITE_BATCH_TIMEOUT_NS =>
            {
                core::hint::spin_loop();
            }
            result => return result,
        }
    }
}

fn queue_blueprint_sprite_batch(
    destination: GpgpuRgba8Surface,
    runs: &[GpgpuSpriteQuadWorklistRun<'_>],
) -> Result<Ui4CompositorSubmission, Ui4CompositorSubmitError> {
    let started = crate::chronos::monotonic_nanos();
    loop {
        match queue_ui4_blueprint_sprite_scene(destination, runs) {
            Err(Ui4CompositorSubmitError::Busy)
            | Err(Ui4CompositorSubmitError::SubmissionRejected)
                if crate::chronos::monotonic_nanos().saturating_sub(started)
                    < UI4_SPRITE_BATCH_TIMEOUT_NS =>
            {
                core::hint::spin_loop();
            }
            result => return result,
        }
    }
}

fn quarantine_blueprint_sprite_submission(
    surface: &mut BlueprintSceneSurface,
    owner: WindowOwner,
    window_id: u32,
    lease: FrameWriteLease,
    batch_index: usize,
    batch_count: usize,
    reason: &'static str,
) {
    surface.gpu_submission_unretired = true;
    crate::log_error!(target: "ui4/blueprint-frame";
        "sprite producer quarantined owner={:?} window={} frame={} buffer={} batch={}/{} reason={} action=no-replay+no-publish+retain-sources+retain-ring-until-engine-reset\n",
        owner,
        window_id,
        lease.frame.raw(),
        lease.buffer_index,
        batch_index.saturating_add(1),
        batch_count,
        reason,
    );
}

fn ensure_solid_source(surface: &mut BlueprintSceneSurface) -> Result<OwnedRgba8Surface, i32> {
    if let Some(source) = surface.solid_source {
        return Ok(source);
    }
    let Some((phys, virt)) =
        crate::dma::alloc(UI4_SCENE_SOLID_SOURCE_BYTES, crate::intel::WARM_ALIGN)
    else {
        return Err(ERROR_UI4);
    };
    unsafe {
        core::ptr::write_bytes(virt, 0, UI4_SCENE_SOLID_SOURCE_BYTES);
        virt.cast::<u32>().write_unaligned(u32::MAX);
    }
    crate::intel::dma_flush(virt, UI4_SCENE_SOLID_SOURCE_BYTES);
    let Some(gpu_surface) =
        GpgpuRgba8Surface::new(phys, UI4_SCENE_SPRITE_GPU, UI4_SCENE_SOLID_SOURCE_BYTES, 1, 1, 64)
    else {
        crate::dma::dealloc(virt, UI4_SCENE_SOLID_SOURCE_BYTES);
        return Err(ERROR_UI4);
    };
    let owned = OwnedRgba8Surface {
        surface: gpu_surface,
        virt,
        bytes: UI4_SCENE_SOLID_SOURCE_BYTES,
    };
    surface.solid_source = Some(owned);
    Ok(owned)
}

fn expected_rgb565_len(width: u32, height: u32) -> Option<usize> {
    (width as usize)
        .checked_mul(height as usize)?
        .checked_mul(core::mem::size_of::<u16>())
}

pub(crate) fn begin_skybox_rgb565_upload(
    owner: WindowOwner,
    window_id: u32,
    width: u32,
    height: u32,
) -> i32 {
    let Some(packed_len) = expected_rgb565_len(width, height) else {
        return ERROR_INVALID;
    };
    let Some(row_bytes) = (width as usize).checked_mul(core::mem::size_of::<u16>()) else {
        return ERROR_INVALID;
    };
    let Some(pitch) =
        crate::intel::align_up(row_bytes, 64).and_then(|pitch| u32::try_from(pitch).ok())
    else {
        return ERROR_INVALID;
    };
    let Some(raw_bytes) = (pitch as usize).checked_mul(height as usize) else {
        return ERROR_INVALID;
    };
    let Some(bytes) = crate::intel::align_up(raw_bytes, crate::intel::WARM_ALIGN) else {
        return ERROR_INVALID;
    };
    if packed_len == 0
        || bytes > UI4_SCENE_SOURCE_MAX_BYTES
        || UI4_SCENE_SOURCE_GPU.saturating_add(bytes as u64)
            > crate::intel::gpgpu::DIRECT_RCS_PPGTT_LIMIT_BYTES
    {
        return ERROR_INVALID;
    }
    let Some((phys, virt)) = crate::dma::alloc(bytes, crate::intel::WARM_ALIGN) else {
        return ERROR_UI4;
    };
    unsafe {
        core::ptr::write_bytes(virt, 0, bytes);
    }
    let Some(gpu_surface) =
        GpgpuRgb565Surface::new(phys, UI4_SCENE_SOURCE_GPU, bytes, width, height, pitch)
    else {
        crate::dma::dealloc(virt, bytes);
        return ERROR_UI4;
    };
    let upload = Rgb565Upload {
        owned: OwnedRgb565Surface {
            surface: gpu_surface,
            virt,
            bytes,
        },
        packed_len,
        written: 0,
    };
    let old = {
        let mut surfaces = SURFACES.lock();
        let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
            drop(surfaces);
            destroy_rgb565_surface(upload.owned);
            return ERROR_NOT_FOUND;
        };
        surface.skybox_upload.replace(upload)
    };
    if let Some(old) = old {
        destroy_rgb565_surface(old.owned);
    }
    0
}

pub(crate) fn write_skybox_rgb565_upload_chunk(
    owner: WindowOwner,
    window_id: u32,
    offset: usize,
    bytes: &[u8],
) -> i32 {
    if bytes.is_empty() {
        return ERROR_INVALID;
    }
    let mut surfaces = SURFACES.lock();
    let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
        return ERROR_NOT_FOUND;
    };
    let Some(upload) = surface.skybox_upload.as_mut() else {
        return ERROR_STATE;
    };
    if offset != upload.written {
        return ERROR_INVALID;
    }
    let Some(end) = offset.checked_add(bytes.len()) else {
        return ERROR_INVALID;
    };
    if end > upload.packed_len {
        return ERROR_INVALID;
    }
    let row_bytes = upload.owned.surface.width as usize * core::mem::size_of::<u16>();
    let pitch = upload.owned.surface.pitch_bytes as usize;
    let mut source_offset = 0usize;
    let mut packed_offset = offset;
    while source_offset < bytes.len() {
        let row = packed_offset / row_bytes;
        let column = packed_offset % row_bytes;
        let count = core::cmp::min(row_bytes - column, bytes.len() - source_offset);
        let destination_offset = row * pitch + column;
        unsafe {
            core::ptr::copy_nonoverlapping(
                bytes.as_ptr().add(source_offset),
                upload.owned.virt.add(destination_offset),
                count,
            );
        }
        source_offset += count;
        packed_offset += count;
    }
    upload.written = end;
    0
}

pub(crate) fn finish_skybox_rgb565_upload(owner: WindowOwner, window_id: u32) -> i32 {
    let upload = {
        let mut surfaces = SURFACES.lock();
        let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
            return ERROR_NOT_FOUND;
        };
        surface.skybox_upload.take()
    };
    let Some(upload) = upload else {
        return ERROR_STATE;
    };
    if upload.written < upload.packed_len {
        destroy_rgb565_surface(upload.owned);
        return ERROR_INVALID;
    }
    crate::intel::dma_flush(upload.owned.virt, upload.owned.bytes);
    let old = {
        let mut surfaces = SURFACES.lock();
        let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
            drop(surfaces);
            destroy_rgb565_surface(upload.owned);
            return ERROR_NOT_FOUND;
        };
        surface.skybox.replace(upload.owned)
    };
    if let Some(old) = old {
        destroy_rgb565_surface(old);
    }
    crate::log_info!(target: "ui4/blueprint-frame"; "skybox source ready owner={:?} window={} extent={}x{} bytes={} gpu=0x{:X}\n", owner, window_id, upload.owned.surface.width, upload.owned.surface.height, upload.owned.bytes, upload.owned.surface.gpu);
    0
}

pub(crate) fn write_opaque_rgba8_chunk(
    owner: WindowOwner,
    window_id: u32,
    offset: usize,
    bytes: &[u8],
) -> i32 {
    if bytes.is_empty()
        || offset & 3 != 0
        || bytes.len() & 3 != 0
        || bytes.chunks_exact(4).any(|pixel| pixel[3] != u8::MAX)
    {
        return ERROR_INVALID;
    }
    let mut surfaces = SURFACES.lock();
    let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
        return ERROR_NOT_FOUND;
    };
    let Some(lease) = surface.write_lease else {
        return ERROR_STATE;
    };
    if surface.gpu_submission_unretired {
        return ERROR_BUSY;
    }
    let Some(expected) = (surface.width as usize)
        .checked_mul(surface.height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
    else {
        return ERROR_INVALID;
    };
    let Some(end) = offset.checked_add(bytes.len()) else {
        return ERROR_INVALID;
    };
    if end > expected {
        return ERROR_INVALID;
    }
    let Ok(view) = writable_rgba_view(lease) else {
        return ERROR_UI4;
    };
    if mark_frame_buffer_cpu_authored(lease).is_err() {
        return ERROR_UI4;
    }
    surface.pending_gpu_release = None;
    surface.sprite_clear_rgba = None;
    surface.sprite_scene_upload = None;
    let row_bytes = surface.width as usize * 4;
    let pitch = view.pitch as usize;
    let mut source_offset = 0usize;
    let mut packed_offset = offset;
    while source_offset < bytes.len() {
        let row = packed_offset / row_bytes;
        let column = packed_offset % row_bytes;
        let count = core::cmp::min(row_bytes - column, bytes.len() - source_offset);
        let destination_offset = row * pitch + column;
        let destination = unsafe { view.virt.add(destination_offset) };
        unsafe {
            core::ptr::copy_nonoverlapping(bytes.as_ptr().add(source_offset), destination, count);
        }
        crate::intel::dma_flush(destination, count);
        source_offset += count;
        packed_offset += count;
    }
    0
}

fn valid_skybox_render_params(params: TrueosUi4SkyboxRenderParams) -> bool {
    let floats = [
        params.right_x,
        params.right_y,
        params.right_z,
        params.up_x,
        params.up_y,
        params.up_z,
        params.forward_x,
        params.forward_y,
        params.forward_z,
        params.aspect_tan_half_fov_y,
        params.tan_half_fov_y,
    ];
    floats.iter().all(|value| value.is_finite())
        && params.aspect_tan_half_fov_y > 0.0
        && params.tan_half_fov_y > 0.0
        && params.rect_width != 0
        && params.rect_height != 0
}

fn destroy_rgb565_surface(surface: OwnedRgb565Surface) {
    crate::dma::dealloc(surface.virt, surface.bytes);
}

fn destroy_rgba8_surface(surface: OwnedRgba8Surface) {
    crate::dma::dealloc(surface.virt, surface.bytes);
}

const fn pack_i32_pair(first: i32, second: i32) -> u64 {
    ((first as u32 as u64) << 32) | second as u32 as u64
}

const fn pack_u32_pair(first: u32, second: u32) -> u64 {
    ((first as u64) << 32) | second as u64
}

fn blueprint_owner() -> Option<WindowOwner> {
    crate::hv::current_guest_execution_context_vm_id().map(WindowOwner::Vm)
}

fn surface_mut(
    surfaces: &mut [BlueprintSceneSurface],
    owner: WindowOwner,
    window_id: u32,
) -> Option<&mut BlueprintSceneSurface> {
    surfaces
        .iter_mut()
        .find(|surface| surface.owner == owner && surface.window.raw() == window_id)
}

fn blueprint_surface_close_request(
    release: BlueprintSurfaceRelease,
) -> WindowSessionCloseRequest<'static> {
    match release {
        BlueprintSurfaceRelease::Animated => {
            WindowSessionCloseRequest::default().direct_plane_animate_and_retire_frames()
        }
        BlueprintSurfaceRelease::AnimatedAndPersistFinalFrame => {
            WindowSessionCloseRequest::default()
                .persist_final_frame()
                .direct_plane_animate_and_retire_frames()
        }
    }
}

fn release_surface(mut surface: BlueprintSceneSurface, release: BlueprintSurfaceRelease) {
    if surface.gpu_submission_unretired {
        let _ = finish_window_session(surface.owner, surface.session);
        crate::log_error!(target: "ui4/blueprint-frame";
            "frame quarantine retained owner={:?} window={} frame={} action=close-window+retain-frame-ring+retain-scene-sources-until-engine-reset\n",
            surface.owner,
            surface.window.raw(),
            surface.frame.raw(),
        );
        QUARANTINED_SURFACES.lock().push(surface);
        return;
    }
    let request = blueprint_surface_close_request(release);
    let pending_immutable_frame = surface
        .write_lease
        .filter(|lease| lease.frame != surface.frame)
        .map(|lease| lease.frame);
    if let Some(lease) = surface.write_lease.take() {
        let _ = cancel_frame_buffer(lease);
    }
    if let Some(pending) = surface.pending_resize.take() {
        let staged_replacement = surface.frame;
        surface.frame = pending.previous_frame;
        if let Err(error) = destroy_frame(staged_replacement)
            && error == FramePoolError::Busy
        {
            RETIRED_FRAMES.lock().push(staged_replacement);
        }
    }
    if let Some(frame) = pending_immutable_frame
        && let Err(error) = destroy_frame(frame)
        && error == FramePoolError::Busy
    {
        RETIRED_FRAMES.lock().push(frame);
    }
    if let Some(upload) = surface.skybox_upload.take() {
        destroy_rgb565_surface(upload.owned);
    }
    if let Some(skybox) = surface.skybox.take() {
        destroy_rgb565_surface(skybox);
    }
    if let Some(upload) = surface.sprite_upload.take() {
        destroy_rgba8_surface(upload.owned);
    }
    for (_, sprite) in surface.sprites.drain(..) {
        destroy_rgba8_surface(sprite);
    }
    if let Some(solid) = surface.solid_source.take() {
        destroy_rgba8_surface(solid);
    }
    let transfer_frame = request.transfers_frame_ownership();
    let close = finish_window_session_with_request(surface.owner, surface.session, request);
    if transfer_frame && close.is_ok() {
        return;
    }
    if let Err(error) = destroy_frame(surface.frame) {
        if error == FramePoolError::Busy {
            RETIRED_FRAMES.lock().push(surface.frame);
        }
        crate::log_warn!(target: "ui4/solara-text"; "frame close deferred owner={:?} window={} error={:?}\n", surface.owner, surface.window.raw(), error);
    }
}

fn reap_retired_frames() {
    RETIRED_FRAMES
        .lock()
        .retain(|frame| matches!(destroy_frame(*frame), Err(FramePoolError::Busy)));
}

fn blend_font_coverage(
    destination: super::FrameRgbaView,
    source: &crate::intel::render::FontRenderTargetReadback,
    dst_x: i32,
    dst_y: i32,
    rgba: u32,
) {
    let [red, green, blue, color_alpha] = rgba.to_le_bytes();
    let destination_len = (destination.pitch as usize)
        .saturating_mul(destination.height as usize)
        .min(destination.byte_len);
    // SAFETY: the caller holds the unique UI4 write lease for this view.
    let destination_pixels =
        unsafe { core::slice::from_raw_parts_mut(destination.virt, destination_len) };
    let source_pitch = source.width as usize * 4;

    for source_y in 0..source.height as usize {
        let target_y = dst_y.saturating_add(source_y as i32);
        if target_y < 0 || target_y >= destination.height as i32 {
            continue;
        }
        for source_x in 0..source.width as usize {
            let target_x = dst_x.saturating_add(source_x as i32);
            if target_x < 0 || target_x >= destination.width as i32 {
                continue;
            }
            let source_offset = source_y * source_pitch + source_x * 4;
            let coverage = source.pixels[source_offset + 3];
            if coverage == 0 {
                continue;
            }
            let source_alpha = mul_div_255(coverage, color_alpha);
            let inverse_alpha = u8::MAX - source_alpha;
            let target_offset =
                target_y as usize * destination.pitch as usize + target_x as usize * 4;
            if target_offset + 4 > destination_pixels.len() {
                continue;
            }
            let target = &mut destination_pixels[target_offset..target_offset + 4];
            target[0] = mul_div_255(red, source_alpha)
                .saturating_add(mul_div_255(target[0], inverse_alpha));
            target[1] = mul_div_255(green, source_alpha)
                .saturating_add(mul_div_255(target[1], inverse_alpha));
            target[2] = mul_div_255(blue, source_alpha)
                .saturating_add(mul_div_255(target[2], inverse_alpha));
            target[3] = source_alpha.saturating_add(mul_div_255(target[3], inverse_alpha));
        }
    }
}

const fn mul_div_255(left: u8, right: u8) -> u8 {
    ((left as u16 * right as u16 + 127) / 255) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keyboard_event(
        seq: u32,
        burst_id: u32,
        flags: u32,
    ) -> crate::r::keyboard::TrueosKeyboardOutputEvent {
        crate::r::keyboard::TrueosKeyboardOutputEvent {
            seq,
            device_seq: burst_id,
            slot_id: 7,
            flags: flags
                | crate::r::keyboard::KEYBOARD_OUTPUT_FLAG_SYNTHETIC
                | crate::r::keyboard::KEYBOARD_OUTPUT_FLAG_TEXT_BURST,
            ..crate::r::keyboard::TrueosKeyboardOutputEvent::default()
        }
    }

    #[test]
    fn keyboard_text_burst_is_hidden_until_complete() {
        let mut pending = VecDeque::new();
        let mut burst = VecDeque::new();

        enqueue_window_keyboard_event(
            &mut pending,
            &mut burst,
            keyboard_event(10, 3, crate::r::keyboard::KEYBOARD_OUTPUT_FLAG_TEXT_BURST_START),
        );
        enqueue_window_keyboard_event(&mut pending, &mut burst, keyboard_event(11, 3, 0));
        assert!(pending.is_empty());
        assert_eq!(burst.len(), 2);

        enqueue_window_keyboard_event(
            &mut pending,
            &mut burst,
            keyboard_event(12, 3, crate::r::keyboard::KEYBOARD_OUTPUT_FLAG_TEXT_BURST_END),
        );
        assert_eq!(pending.len(), 3);
        assert!(burst.is_empty());
    }

    #[test]
    fn keyboard_text_burst_with_a_sequence_gap_is_discarded() {
        let mut pending = VecDeque::new();
        let mut burst = VecDeque::new();

        enqueue_window_keyboard_event(
            &mut pending,
            &mut burst,
            keyboard_event(20, 4, crate::r::keyboard::KEYBOARD_OUTPUT_FLAG_TEXT_BURST_START),
        );
        enqueue_window_keyboard_event(
            &mut pending,
            &mut burst,
            keyboard_event(22, 4, crate::r::keyboard::KEYBOARD_OUTPUT_FLAG_TEXT_BURST_END),
        );

        assert!(pending.is_empty());
        assert!(burst.is_empty());
    }

    fn retained_description(y: f32) -> BlueprintRetainedTextDescription {
        BlueprintRetainedTextDescription {
            font: GpuFontFace::Inconsolata,
            viewport_width: 960,
            viewport_height: 720,
            runs: alloc::vec![
                RetainedFontRun {
                    text: String::from("first"),
                    position: [12.0, y],
                    font_pixels: 14.0,
                    slant: 0.0,
                },
                RetainedFontRun {
                    text: String::from("second"),
                    position: [12.0, y + 18.0],
                    font_pixels: 14.0,
                    slant: 0.0,
                },
            ],
        }
    }

    #[test]
    fn retained_text_treats_only_empty_coverage_as_a_transparent_noop() {
        assert!(retained_text_error_is_no_coverage(FontKernelError::Unavailable(
            "font-coverage-empty"
        )));
        assert!(!retained_text_error_is_no_coverage(FontKernelError::Unavailable(
            "font-coverage-workload"
        )));
        assert!(!retained_text_error_is_no_coverage(FontKernelError::SubmittedIncomplete(
            "font-coverage-submit-incomplete"
        )));

        let layer = BlueprintRetainedTextLayer {
            description: retained_description(40.0),
            color_rgba: u32::MAX,
            translation_px: [12, 18],
            state: BlueprintRetainedTextState::NoCoverage,
        };
        assert!(matches!(retained_text_scene(&layer.state), Ok(None)));
    }

    #[test]
    fn blueprint_surface_close_uses_direct_plane_scaling_and_preserves_capture() {
        assert_eq!(
            blueprint_surface_close_request(BlueprintSurfaceRelease::Animated),
            WindowSessionCloseRequest::default().direct_plane_animate_and_retire_frames(),
        );
        assert_eq!(
            blueprint_surface_close_request(BlueprintSurfaceRelease::AnimatedAndPersistFinalFrame,),
            WindowSessionCloseRequest::default()
                .persist_final_frame()
                .direct_plane_animate_and_retire_frames(),
        );
    }

    #[test]
    fn retained_text_reuses_integral_scene_translation() {
        assert_eq!(
            retained_description(40.0).translation_to(&retained_description(-24.0)),
            Some([0, -64]),
        );
    }

    #[test]
    fn retained_text_rejects_content_and_subpixel_changes() {
        let base = retained_description(40.0);
        let mut changed = retained_description(40.0);
        changed.runs[1].text = String::from("different");
        assert_eq!(base.translation_to(&changed), None);
        assert_eq!(base.translation_to(&retained_description(40.5)), None,);
    }

    #[test]
    fn font_canvas_hides_color_layers_behind_one_stamp_request() {
        let description = BlueprintFontCanvasDescription {
            font: GpuFontFace::Inconsolata,
            width: 960,
            height: 720,
            rows: alloc::vec![
                BlueprintFontCanvasRow {
                    text: String::from("title"),
                    position: [24.0, 24.0],
                    font_pixels: 28.0,
                    color_rgba: u32::from_le_bytes([255, 255, 255, 255]),
                },
                BlueprintFontCanvasRow {
                    text: String::from("detail"),
                    position: [24.0, 64.0],
                    font_pixels: 18.0,
                    color_rgba: u32::from_le_bytes([128, 160, 192, 255]),
                },
                BlueprintFontCanvasRow {
                    text: String::from("same title color"),
                    position: [24.0, 96.0],
                    font_pixels: 18.0,
                    color_rgba: u32::from_le_bytes([255, 255, 255, 255]),
                },
            ],
        };

        let request = font_canvas_request(&description);

        assert_eq!(request.fit, FontStampFit::Canvas);
        assert_eq!(request.layers.len(), 2);
        assert_eq!(request.layers[0].scene.runs.len(), 2);
        assert_eq!(request.layers[1].scene.runs.len(), 1);
        assert!(request.layers.iter().all(|layer| {
            layer.scene.raster_width == 960
                && layer.scene.raster_height == 720
                && layer.scene.positioning == RetainedFontPositioning::SceneOrigin
        }));
    }

    #[test]
    fn font_canvas_splits_large_same_color_run_sets_only_inside_stamp() {
        let rows = (0..65)
            .map(|index| BlueprintFontCanvasRow {
                text: alloc::format!("row {index}"),
                position: [12.0, index as f32 * 16.0],
                font_pixels: 14.0,
                color_rgba: u32::from_le_bytes([255, 255, 255, 255]),
            })
            .collect::<Vec<_>>();
        let description = BlueprintFontCanvasDescription {
            font: GpuFontFace::Inconsolata,
            width: 960,
            height: 1_200,
            rows,
        };

        let request = font_canvas_request(&description);

        assert_eq!(font_canvas_internal_layer_count(&description.rows), 2);
        assert_eq!(request.layers.len(), 2);
        assert_eq!(request.layers[0].scene.runs.len(), 64);
        assert_eq!(request.layers[1].scene.runs.len(), 1);
    }

    #[test]
    fn font_canvas_quad_keeps_premultiplied_source_over_math() {
        let quad = TrueosUi4SpriteQuad {
            flags: SPRITE_QUAD_FLAG_SRC_OVER,
            ..TrueosUi4SpriteQuad::default()
        };

        let descriptor = gpgpu_sprite_quad_descriptor(quad, true);

        assert_eq!(
            descriptor.flags,
            SPRITE_QUAD_WORKLIST_FLAG_SRC_OVER | SPRITE_QUAD_WORKLIST_FLAG_PREMUL_SRC,
        );
    }
}
