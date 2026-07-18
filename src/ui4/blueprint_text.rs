//! Experimental Blueprint transport for Solara's text-only UI4 producer.
//!
//! The kernel derives window ownership from the active Blueprint VM. Callers
//! provide text and placement only; frame handles and writable surfaces never
//! cross the ABI boundary.

use alloc::{string::String, vec::Vec};
use spin::Mutex;

use crate::intel::gpu_font::{
    GpuFontFace, GpuFontJob, GpuFontJobEntry, GpuFontTextRequest, MAX_DYNAMIC_TEXT_CHARS,
    ensure_font_face_available, recycle_font_job_readback, render_font_job_readback_once,
    render_font_scene_readback_once,
};

use super::{
    DamageRect, FrameCadence, FrameContent, FrameHandle, FramePoolError, FrameSpec,
    FrameWriteLease, OutputId, PremultipliedRgba8, ScanoutFormat, WindowCreate, WindowId,
    WindowOwner, WindowPlacement, WindowPlane, WindowSessionCloseRequest, WindowSessionId,
    acquire_frame_buffer, begin_window_session, cancel_frame_buffer, create_frame, create_window,
    destroy_frame, finish_window_session, finish_window_session_with_request, publish_frame_buffer,
    publish_window_frame, writable_rgba_view,
};

const MAX_SURFACES: usize = 32;
const MAX_FRAME_WIDTH: u32 = 2_560;
const MAX_FRAME_HEIGHT: u32 = 1_440;
const MAX_TEXT_ROWS: usize = 64;
const MAX_TEXT_ROW_BYTES: usize = 1_024;
const MAX_NATIVE_FONT_SIZES: usize = 32;
const TEXT_ROWS_WIRE_HEADER_BYTES: usize = 16;
const TEXT_ROW_WIRE_HEADER_BYTES: usize = 12;
const TEXT_SCENE_WIRE_HEADER_BYTES: usize = 16;
const TEXT_SCENE_ROW_WIRE_HEADER_BYTES: usize = 16;

const ERROR_INVALID: i32 = -1;
const ERROR_CONTEXT: i32 = -2;
const ERROR_NOT_FOUND: i32 = -3;
const ERROR_STATE: i32 = -4;
const ERROR_FONT: i32 = -5;
const ERROR_UI4: i32 = -6;
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

struct SolaraTextSurface {
    owner: WindowOwner,
    session: WindowSessionId,
    frame: FrameHandle,
    window: WindowId,
    width: u32,
    height: u32,
    write_lease: Option<FrameWriteLease>,
}

static SURFACES: Mutex<Vec<SolaraTextSurface>> = Mutex::new(Vec::new());
static RETIRED_FRAMES: Mutex<Vec<FrameHandle>> = Mutex::new(Vec::new());

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
    reap_retired_frames();
    let Some(owner) = blueprint_owner() else {
        return 0;
    };

    let old = {
        let mut surfaces = SURFACES.lock();
        surfaces
            .iter()
            .position(|surface| surface.owner == owner)
            .map(|slot| surfaces.remove(slot))
    };
    if let Some(old) = old {
        release_surface(old);
    }

    let output = OutputId::from_slot(0).expect("UI4 D01 must exist");
    let frame = match create_frame(FrameSpec {
        output,
        content: FrameContent::FontScene2d,
        cadence: FrameCadence::Dirty,
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
    let session = match begin_window_session(owner) {
        Ok(session) => session,
        Err(error) => {
            let _ = destroy_frame(frame);
            crate::log_error!(target: "ui4/solara-text"; "frame open session failed owner={:?} error={:?}\n", owner, error);
            return 0;
        }
    };
    let window = match create_window(WindowCreate {
        owner,
        session,
        frame,
        output,
        plane: WindowPlane::Universal(super::RGB_OVERLAY_PLANE_SLOT_2 as u8),
        placement: WindowPlacement {
            x,
            y,
            width,
            height,
            z: 40,
            opacity: u8::MAX,
            visible: true,
        },
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
        release_surface(SolaraTextSurface {
            owner,
            session,
            frame,
            window,
            width,
            height,
            write_lease: None,
        });
        return 0;
    }
    surfaces.push(SolaraTextSurface {
        owner,
        session,
        frame,
        window,
        width,
        height,
        write_lease: None,
    });
    crate::log_info!(target: "ui4/solara-text"; "frame open owner={:?} window={} extent={}x{} cadence=dirty buffers=2 plane=slot2 text_path=opaque-glyph-mask\n", owner, window.raw(), width, height);
    window.raw()
}

/// Acquire and clear the non-front UI4 buffer for a new Solara paint pass.
pub extern "C" fn trueos_cabi_ui4_solara_frame_begin(window_id: u32, clear_rgba: u32) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return guest_status(
            trueos_vm::vmcall::OP_BP_UI4_SOLARA_FRAME_BEGIN,
            window_id as u64,
            clear_rgba as u64,
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
    if surface.write_lease.is_some() {
        return ERROR_STATE;
    }
    let lease = match acquire_frame_buffer(surface.frame) {
        Ok(lease) => lease,
        Err(_) => return ERROR_UI4,
    };
    let view = match writable_rgba_view(lease) {
        Ok(view) => view,
        Err(_) => {
            let _ = cancel_frame_buffer(lease);
            return ERROR_UI4;
        }
    };
    let [r, g, b, a] = clear_rgba.to_le_bytes();
    let pixel = PremultipliedRgba8::from_straight_rgba(r, g, b, a).to_native_bytes();
    // SAFETY: the write lease makes this entire UI4 allocation producer-owned.
    let bytes = unsafe { core::slice::from_raw_parts_mut(view.virt, view.byte_len) };
    for chunk in bytes.chunks_exact_mut(4) {
        chunk.copy_from_slice(&pixel);
    }
    crate::intel::dma_flush(view.virt, view.byte_len);
    surface.write_lease = Some(lease);
    0
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
    if publish_frame_buffer(lease).is_err() {
        return ERROR_UI4;
    }
    if damage.width == 0
        || damage.height == 0
        || publish_window_frame(owner, surface.window, damage).is_err()
    {
        return ERROR_UI4;
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
    let request = if flags & CLOSE_PERSIST_FINAL_FRAME != 0 {
        WindowSessionCloseRequest::default()
            .persist_final_frame()
            .animate_and_retire_frames()
    } else {
        WindowSessionCloseRequest::default().animate_and_retire_frames()
    };
    release_surface_with_request(surface, request);
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
    surfaces: &mut [SolaraTextSurface],
    owner: WindowOwner,
    window_id: u32,
) -> Option<&mut SolaraTextSurface> {
    surfaces
        .iter_mut()
        .find(|surface| surface.owner == owner && surface.window.raw() == window_id)
}

fn release_surface(surface: SolaraTextSurface) {
    release_surface_with_request(surface, WindowSessionCloseRequest::default());
}

fn release_surface_with_request(
    mut surface: SolaraTextSurface,
    request: WindowSessionCloseRequest<'_>,
) {
    if let Some(lease) = surface.write_lease.take() {
        let _ = cancel_frame_buffer(lease);
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
