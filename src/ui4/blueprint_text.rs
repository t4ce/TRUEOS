//! Blueprint transport for VM-owned UI4 scene frames.
//!
//! The kernel derives window ownership from the active Blueprint VM. Callers
//! provide scene operations and placement only; frame handles, writable
//! surfaces, and GPU addresses never cross the ABI boundary. Solara's text
//! producer was the first consumer; shaded scene producers share the same
//! coherent UI4 frame lifecycle.

use alloc::{collections::VecDeque, string::String, vec::Vec};
use spin::Mutex;

use crate::intel::gpgpu::{
    ALPHA_BLEND_WORKLIST_FLAG_COPY, ALPHA_BLEND_WORKLIST_FLAG_SRC_OVER,
    ALPHA_BLEND_WORKLIST_FLAG_TINT_ALPHA, ALPHA_BLEND_WORKLIST_FLAG_TINT_RGB,
    GpgpuAlphaBlendWorklistDesc, GpgpuRgb565Surface, GpgpuRgba8ReleaseFence, GpgpuRgba8Surface,
    GpgpuSpriteQuadWorklistDesc, GpgpuSpriteQuadWorklistRun, SPRITE_QUAD_WORKLIST_FLAG_SRC_OVER,
    SkyboxSampleRgb565Params, Ui4CompositorCompletion, Ui4CompositorSubmission,
    Ui4CompositorSubmitError, Ui4SpriteSceneCompletion, alpha_blend_worklist_max_descs,
    poll_ui4_blueprint_sprite_scene, poll_ui4_compositor_submission,
    queue_ui4_blueprint_alpha_rects, queue_ui4_blueprint_sprite_scene,
    skybox_sample_rgb565_to_rgba8, sprite_quad_worklist_max_descs,
};
use crate::intel::gpu_font::{
    GpuFontFace, GpuFontJob, GpuFontJobEntry, GpuFontTextRequest, MAX_DYNAMIC_TEXT_CHARS,
    ensure_font_face_available, recycle_font_job_readback, render_font_job_readback_once,
    render_font_scene_readback_once,
};

use super::{
    DamageRect, FrameCadence, FrameContent, FrameHandle, FramePoolError, FrameSpec,
    FrameWriteLease, OutputId, PremultipliedRgba8, ScanoutFormat, Ui4CursorIcon, Ui4CursorSource,
    Ui4InputEvent, WindowCreate, WindowId, WindowOwner, WindowPlacement, WindowPlane,
    WindowSessionCloseRequest, WindowSessionId, acquire_frame_buffer,
    begin_additional_window_session, cancel_frame_buffer, create_frame, create_window,
    destroy_frame, finish_window_session, finish_window_session_with_request,
    focused_keyboard_state, gpgpu_rgba_surface, mark_frame_buffer_cpu_authored,
    publish_frame_buffer, publish_gpgpu_scene_frame_buffer, publish_window_frame,
    replace_window_frame, set_window_cursor_icon, set_window_custom_cursor, set_window_placement,
    take_owner_input_events, window_placement, writable_rgba_view,
};

const MAX_SURFACES: usize = 32;
const MAX_FRAME_WIDTH: u32 = 2_560;
const MAX_FRAME_HEIGHT: u32 = 1_440;
const MAX_TEXT_ROWS: usize = 64;
const MAX_TEXT_ROW_BYTES: usize = 1_024;
const MAX_NATIVE_FONT_SIZES: usize = 32;
const MAX_PENDING_POINTER_EVENTS: usize = 256;
const MAX_PENDING_PAN_EVENTS: usize = 256;
const TEXT_ROWS_WIRE_HEADER_BYTES: usize = 16;
const TEXT_ROW_WIRE_HEADER_BYTES: usize = 12;
const TEXT_SCENE_WIRE_HEADER_BYTES: usize = 16;
const TEXT_SCENE_ROW_WIRE_HEADER_BYTES: usize = 16;
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

const ERROR_INVALID: i32 = -1;
const ERROR_CONTEXT: i32 = -2;
const ERROR_NOT_FOUND: i32 = -3;
const ERROR_STATE: i32 = -4;
const ERROR_FONT: i32 = -5;
const ERROR_UI4: i32 = -6;
const ERROR_BUSY: i32 = -7;
const CLOSE_PERSIST_FINAL_FRAME: u32 = 1 << 0;
const CLOSE_VALID_FLAGS: u32 = CLOSE_PERSIST_FINAL_FRAME;

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

/// One broker-requested frame extent transition. The Blueprint may ignore the
/// event and retain its old 1:1 backing allocation, or replace that allocation
/// through `trueos_cabi_ui4_scene_frame_resize`.
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
    write_lease: Option<FrameWriteLease>,
    pending_gpu_release: Option<GpgpuRgba8ReleaseFence>,
    gpu_submission_unretired: bool,
    placement: WindowPlacement,
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
    open_blueprint_frame(x, y, width, height, FrameCadence::Dirty)
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
    open_blueprint_frame(x, y, width, height, FrameCadence::Immutable)
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
    open_blueprint_frame(x, y, width, height, FrameCadence::Streaming)
}

fn open_blueprint_frame(x: i32, y: i32, width: u32, height: u32, cadence: FrameCadence) -> u32 {
    reap_retired_frames();
    let Some(owner) = blueprint_owner() else {
        return 0;
    };

    let output = OutputId::from_slot(0).expect("UI4 D01 must exist");
    let frame = match create_frame(FrameSpec {
        output,
        content: FrameContent::BlueprintScene,
        cadence,
        buffering: match cadence {
            FrameCadence::Immutable => super::FrameBuffering::Single,
            FrameCadence::Dirty => super::FrameBuffering::Double,
            FrameCadence::Streaming => super::FrameBuffering::Triple,
        },
        format: ScanoutFormat::Rgba8888Premultiplied,
        width,
        height,
        base_color: Some(PremultipliedRgba8::TRANSPARENT),
    }) {
        Ok(frame) => frame,
        Err(error) => {
            crate::log_error!(target: "ui4/solara-text"; "frame open allocation failed owner={:?} error={:?}\n", owner, error);
            return 0;
        }
    };
    let session = match begin_additional_window_session(owner) {
        Ok(session) => session,
        Err(error) => {
            let _ = destroy_frame(frame);
            crate::log_error!(target: "ui4/solara-text"; "frame open session failed owner={:?} error={:?}\n", owner, error);
            return 0;
        }
    };
    let placement = WindowPlacement {
        x,
        y,
        width,
        height,
        z: 40,
        opacity: u8::MAX,
        visible: true,
    };
    let plane_slot = if cadence == FrameCadence::Streaming {
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
            crate::log_error!(target: "ui4/solara-text"; "frame open window failed owner={:?} error={:?}\n", owner, error);
            return 0;
        }
    };

    let mut surfaces = SURFACES.lock();
    if surfaces.len() >= MAX_SURFACES {
        drop(surfaces);
        release_surface(
            BlueprintSceneSurface {
                owner,
                session,
                frame,
                window,
                width,
                height,
                cadence,
                write_lease: None,
                pending_gpu_release: None,
                gpu_submission_unretired: false,
                placement,
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
        cadence,
        write_lease: None,
        pending_gpu_release: None,
        gpu_submission_unretired: false,
        placement,
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
    });
    let (cadence_name, buffer_count) = match cadence {
        FrameCadence::Dirty => ("dirty", 2),
        FrameCadence::Streaming => ("streaming", 3),
        FrameCadence::Immutable => ("immutable", 1),
    };
    crate::log_info!(target: "ui4/blueprint-frame"; "frame open owner={:?} window={} extent={}x{} cadence={} buffers={} plane=slot{} scene=text+shader\n", owner, window.raw(), width, height, cadence_name, buffer_count, plane_slot);
    window.raw()
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
    }
    surface.pending_gpu_release = None;
    surface.sprite_clear_rgba = (!cpu_clear).then_some(clear_rgba);
    surface.sprite_scene_upload = None;
    surface.write_lease = Some(lease);
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

/// Drain one VM owner's broker queue once and preserve every pointer, pan, and
/// resize event for its exact surface. Keyboard held state remains in the input
/// broker; draining its cooked events here prevents an interactive Blueprint
/// from filling the owner queue while it polls the selected-frame contracts.
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
            _ => {}
        }
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
    let (cadence, window) = {
        let mut surfaces = SURFACES.lock();
        let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
            return ERROR_NOT_FOUND;
        };
        if surface.write_lease.is_some() {
            return ERROR_STATE;
        }
        (surface.cadence, surface.window)
    };
    let output = OutputId::from_slot(0).expect("UI4 D01 must exist");
    let replacement = match create_frame(FrameSpec {
        output,
        content: FrameContent::BlueprintScene,
        cadence,
        buffering: match cadence {
            FrameCadence::Immutable => super::FrameBuffering::Single,
            FrameCadence::Dirty => super::FrameBuffering::Double,
            FrameCadence::Streaming => super::FrameBuffering::Triple,
        },
        format: ScanoutFormat::Rgba8888Premultiplied,
        width,
        height,
        base_color: Some(PremultipliedRgba8::TRANSPARENT),
    }) {
        Ok(frame) => frame,
        Err(_) => return ERROR_UI4,
    };

    let live_placement = match window_placement(owner, window) {
        Ok(placement) => placement,
        Err(_) => {
            let _ = destroy_frame(replacement);
            return ERROR_UI4;
        }
    };
    let previous = {
        let mut surfaces = SURFACES.lock();
        let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
            let _ = destroy_frame(replacement);
            return ERROR_NOT_FOUND;
        };
        if surface.write_lease.is_some() {
            let _ = destroy_frame(replacement);
            return ERROR_STATE;
        }
        let previous = surface.frame;
        if replace_window_frame(owner, surface.window, replacement).is_err() {
            let _ = destroy_frame(replacement);
            return ERROR_UI4;
        }
        let placement = WindowPlacement {
            width,
            height,
            ..live_placement
        };
        if set_window_placement(owner, surface.window, placement).is_err() {
            let _ = replace_window_frame(owner, surface.window, previous);
            let _ = destroy_frame(replacement);
            return ERROR_UI4;
        }
        surface.frame = replacement;
        surface.width = width;
        surface.height = height;
        surface.placement = placement;
        previous
    };

    if let Err(error) = destroy_frame(previous) {
        if error == FramePoolError::Busy {
            RETIRED_FRAMES.lock().push(previous);
        }
    }
    crate::log_info!(target: "ui4/blueprint-frame"; "frame resize owner={:?} window={} extent={}x{} frame={}\n", owner, window_id, width, height, replacement.raw());
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
    let Some(font) = GpuFontFace::from_id(font_id) else {
        return ERROR_FONT;
    };
    if let Err(reason) = ensure_font_face_available(font) {
        crate::log_warn!(target: "ui4/solara-text"; "font unavailable owner={:?} window={} font={} reason={}\n", owner, window_id, font.registry_name(), reason);
        return ERROR_FONT;
    }
    if rows.is_null() || row_count == 0 || row_count > MAX_TEXT_ROWS {
        return ERROR_INVALID;
    }
    {
        let mut surfaces = SURFACES.lock();
        let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
            return ERROR_NOT_FOUND;
        };
        if surface.width != viewport_width || surface.height != viewport_height {
            return ERROR_INVALID;
        }
        if surface.write_lease.is_none() {
            return ERROR_STATE;
        }
    }

    let input = unsafe { core::slice::from_raw_parts(rows, row_count) };
    let mut strings = Vec::<String>::with_capacity(row_count);
    let mut positions = Vec::<[f32; 2]>::with_capacity(row_count);
    let mut font_pixels = Vec::<f32>::with_capacity(row_count);
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
        strings.push(String::from(text));
        positions.push([row.x, row.y]);
        font_pixels.push(row.font_pixels);
    }
    let entries: Vec<_> = strings
        .iter()
        .zip(positions.iter())
        .zip(font_pixels.iter())
        .map(|((text, position), font_pixels)| GpuFontJobEntry {
            text: GpuFontTextRequest::SingleLine(text.as_str()),
            position: *position,
            font_pixels: *font_pixels,
            slant: 0.0,
        })
        .collect();
    render_scene_entries_into_surface(
        owner,
        window_id,
        font,
        viewport_width,
        viewport_height,
        rgba,
        entries.as_slice(),
    )
}

fn render_scene_entries_into_surface(
    owner: WindowOwner,
    window_id: u32,
    font: GpuFontFace,
    viewport_width: u32,
    viewport_height: u32,
    rgba: u32,
    entries: &[GpuFontJobEntry<'_>],
) -> i32 {
    let readback = match render_font_scene_readback_once(
        GpuFontJob {
            entries,
            font,
            native_scale: 1,
        },
        viewport_width,
        viewport_height,
    ) {
        Ok(readback) => readback,
        Err("font-mesh-staging-capacity") if entries.len() > 1 => {
            let middle = entries.len() / 2;
            crate::log_info!(target: "ui4/solara-text"; "scene split owner={:?} window={} entries={} left={} right={} reason=bounded-transient-staging\n", owner, window_id, entries.len(), middle, entries.len() - middle);
            let left = render_scene_entries_into_surface(
                owner,
                window_id,
                font,
                viewport_width,
                viewport_height,
                rgba,
                &entries[..middle],
            );
            if left != 0 {
                return left;
            }
            return render_scene_entries_into_surface(
                owner,
                window_id,
                font,
                viewport_width,
                viewport_height,
                rgba,
                &entries[middle..],
            );
        }
        Err(reason) => {
            crate::log_warn!(target: "ui4/solara-text"; "scene render rejected owner={:?} window={} entries={} reason={}\n", owner, window_id, entries.len(), reason);
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
                write_font_opaque_mask(view, &readback, 0, 0, rgba);
                crate::intel::dma_flush(view.virt, view.byte_len);
                0
            }
            Err(_) => ERROR_UI4,
        }
    };
    recycle_font_job_readback(readback);
    result
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
    let release = surface.pending_gpu_release.take();
    let publish = match release {
        Some(release) => publish_gpgpu_scene_frame_buffer(lease, release),
        None => publish_frame_buffer(lease),
    };
    if publish.is_err() {
        surface.write_lease = Some(lease);
        surface.pending_gpu_release = release;
        return ERROR_UI4;
    }
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
    0
}

pub extern "C" fn trueos_cabi_ui4_solara_frame_close(window_id: u32) -> i32 {
    trueos_cabi_ui4_solara_frame_close_requested(window_id, 0)
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
    guest_status(
        trueos_vm::vmcall::OP_BP_UI4_SOLARA_TEXT_SCENE,
        window_id as u64,
        font_id as u64,
        payload.as_slice(),
    )
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
    if sprite_id == 0 || packed_len == 0 {
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

    let gpu = {
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
            return ERROR_UI4;
        };
        gpu
    };
    let Some((phys, virt)) = crate::dma::alloc(bytes, crate::intel::WARM_ALIGN) else {
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
        "sprite source ready owner={:?} window={} sprite={} extent={}x{} bytes={} gpu=0x{:X}\n",
        owner,
        window_id,
        sprite_id,
        upload.owned.surface.width,
        upload.owned.surface.height,
        upload.owned.bytes,
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

fn gpgpu_sprite_quad_descriptor(quad: TrueosUi4SpriteQuad) -> GpgpuSpriteQuadWorklistDesc {
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

    let mut surfaces = SURFACES.lock();
    let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
        return ERROR_NOT_FOUND;
    };
    let Some(upload) = surface.sprite_scene_upload.take() else {
        return ERROR_STATE;
    };
    if upload.quads.len() != upload.expected {
        return ERROR_INVALID;
    }
    let Some(lease) = surface.write_lease else {
        return ERROR_STATE;
    };
    let Some(clear_rgba) = surface.sprite_clear_rgba else {
        return ERROR_STATE;
    };
    if surface.gpu_submission_unretired {
        return ERROR_BUSY;
    }
    let solid = match ensure_solid_source(surface) {
        Ok(source) => source,
        Err(code) => return code,
    };
    let Ok(destination) = gpgpu_rgba_surface(lease) else {
        return ERROR_UI4;
    };

    let retry_quads = upload.quads.clone();
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
    prepared.push(PreparedOp::Quad {
        sprite_id: 0,
        source: solid.surface,
        descriptor: gpgpu_sprite_quad_descriptor(clear),
    });
    for quad in upload.quads {
        let source = if quad.sprite_id == 0 {
            solid.surface
        } else {
            let Some((_, source)) = surface
                .sprites
                .iter()
                .find(|(sprite_id, _)| *sprite_id == quad.sprite_id)
            else {
                return ERROR_NOT_FOUND;
            };
            source.surface
        };
        let conversion = if quad.sprite_id == 0 {
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
                descriptor: gpgpu_sprite_quad_descriptor(quad),
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
                surface.sprite_scene_upload = Some(SpriteSceneUpload {
                    expected: retry_quads.len(),
                    quads: retry_quads.clone(),
                });
                crate::log_warn!(target: "ui4/blueprint-frame";
                    "sprite scene deferred owner={:?} window={} frame={} buffer={} batch={}/{} descriptors={} backend={} reason=ui4-compositor-admission-timeout hardware_accepted=0 action=retain-upload+retry-finish\n",
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
                crate::log_warn!(target: "ui4/blueprint-frame";
                    "sprite scene rejected owner={:?} window={} batch={}/{} backend={} reason=ui4-compositor-unavailable hardware_accepted=0\n",
                    owner,
                    window_id,
                    batch_index.saturating_add(1),
                    batch_count,
                    backend,
                );
                return ERROR_UI4;
            }
            Err(Ui4CompositorSubmitError::InvalidWorklist) => {
                crate::log_error!(target: "ui4/blueprint-frame";
                    "sprite scene rejected owner={:?} window={} batch={}/{} descriptors={} backend={} reason=invalid-private-ui4-worklist hardware_accepted=0\n",
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
                            crate::log_error!(target: "ui4/blueprint-frame";
                                "sprite scene retirement contract mismatch owner={:?} window={} batch={}/{} expected_descs={} retired_descs={} retired_submits={} action=reject-release\n",
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
                        return ERROR_BUSY;
                    }
                }
            } else {
                match poll_ui4_compositor_submission(submission) {
                    Ui4CompositorCompletion::Pending => false,
                    Ui4CompositorCompletion::Complete(stats) => {
                        if stats.descs != descriptor_count || stats.submits != 1 {
                            crate::log_error!(target: "ui4/blueprint-frame";
                                "sprite scene retirement contract mismatch owner={:?} window={} batch={}/{} expected_descs={} retired_descs={} retired_submits={} action=abort-before-release\n",
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
                        return ERROR_BUSY;
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
                return ERROR_BUSY;
            }
            core::hint::spin_loop();
        }
    }
    let Some(final_release) = final_release else {
        return ERROR_UI4;
    };
    surface.pending_gpu_release = Some(final_release);
    surface.sprite_clear_rgba = None;
    0
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
    let request = match release {
        BlueprintSurfaceRelease::Animated => {
            WindowSessionCloseRequest::default().animate_and_retire_frames()
        }
        BlueprintSurfaceRelease::AnimatedAndPersistFinalFrame => {
            WindowSessionCloseRequest::default()
                .persist_final_frame()
                .animate_and_retire_frames()
        }
    };
    let pending_immutable_frame = surface
        .write_lease
        .filter(|lease| lease.frame != surface.frame)
        .map(|lease| lease.frame);
    if let Some(lease) = surface.write_lease.take() {
        let _ = cancel_frame_buffer(lease);
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

/// Write a kernel font-scene mask into a UI4 frame as opaque pixels.
///
/// The transient font target still supplies the glyph mask, but none of its
/// coverage alpha crosses into the UI4 frame. This deliberately trades edge
/// antialiasing for an unambiguous opaque-pixel presentation path. Solara and
/// small kernel-owned proof surfaces share this helper; the legacy text-row
/// ABI keeps using `blend_font_coverage` and retains alpha-capable behavior.
pub(crate) fn write_font_opaque_mask(
    destination: super::FrameRgbaView,
    source: &crate::intel::render::FontRenderTargetReadback,
    dst_x: i32,
    dst_y: i32,
    rgba: u32,
) {
    let [red, green, blue, _] = rgba.to_le_bytes();
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
            if source.pixels[source_offset + 3] == 0 {
                continue;
            }
            let target_offset =
                target_y as usize * destination.pitch as usize + target_x as usize * 4;
            if target_offset + 4 > destination_pixels.len() {
                continue;
            }
            destination_pixels[target_offset..target_offset + 4].copy_from_slice(&[
                red,
                green,
                blue,
                u8::MAX,
            ]);
        }
    }
}

const fn mul_div_255(left: u8, right: u8) -> u8 {
    ((left as u16 * right as u16 + 127) / 255) as u8
}
