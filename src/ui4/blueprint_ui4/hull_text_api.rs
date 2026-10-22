//! Hull's bounded text-only bridge. Guest pointers are copied into a wire
//! payload; the host only parses owned UTF-8 and rechecks the exact write lease.

use super::{
    ERROR_BUSY, ERROR_CONTEXT, ERROR_INVALID, ERROR_NOT_FOUND, ERROR_STATE, ERROR_UI4, SURFACES,
    WindowOwner, blueprint_owner, guest_status, surface_mut,
};
use crate::r::services::hull_font_service::{self, HullTextRun};
use alloc::{string::String, vec::Vec};
use v::bp_abi::TrueosUi4HullTextRunV1;

const MAX_RUNS: usize = 2_048;
const MAX_RUN_TEXT_BYTES: usize = 4_096;
const MAX_TEXT_BYTES: usize = 64 * 1024;
const WIRE_RUN_BYTES: usize = 28;

struct PaintFlushRegion {
    offset: usize,
    row_bytes: usize,
    rows: usize,
}

/// Validate the clipped paint union against both the visible extent and actual
/// pitched allocation before constructing an interior DMA pointer.
fn paint_flush_region(
    width: u32,
    height: u32,
    pitch: u32,
    byte_len: usize,
    bounds: Option<(u32, u32, u32, u32)>,
) -> Result<Option<PaintFlushRegion>, i32> {
    let Some((x, y, dirty_width, dirty_height)) = bounds else {
        return Ok(None);
    };
    if dirty_width == 0
        || dirty_height == 0
        || x.checked_add(dirty_width).is_none_or(|right| right > width)
        || y.checked_add(dirty_height)
            .is_none_or(|bottom| bottom > height)
        || (width as usize)
            .checked_mul(4)
            .is_none_or(|row| row > pitch as usize)
    {
        return Err(ERROR_UI4);
    }
    let row_bytes = (dirty_width as usize).checked_mul(4).ok_or(ERROR_UI4)?;
    let offset = (y as usize)
        .checked_mul(pitch as usize)
        .and_then(|row| {
            (x as usize)
                .checked_mul(4)
                .and_then(|column| row.checked_add(column))
        })
        .ok_or(ERROR_UI4)?;
    let end = (dirty_height as usize - 1)
        .checked_mul(pitch as usize)
        .and_then(|last_row| offset.checked_add(last_row))
        .and_then(|last_row| last_row.checked_add(row_bytes))
        .ok_or(ERROR_UI4)?;
    if end > byte_len {
        return Err(ERROR_UI4);
    }
    Ok(Some(PaintFlushRegion {
        offset,
        row_bytes,
        rows: dirty_height as usize,
    }))
}

fn valid_geometry(x: f32, y: f32, size: f32, advance: f32) -> bool {
    x.is_finite()
        && y.is_finite()
        && size.is_finite()
        && advance.is_finite()
        && x.abs() <= 1_000_000.0
        && y.abs() <= 1_000_000.0
        && size > 0.0
        && size <= 1_440.0
        && advance >= 0.0
        && advance <= 1_000_000.0
}

/// Overlay after the preceding sprite draw has synchronously retired and before
/// publication. No guest pointer, font allocation, or raster work holds SURFACES.
pub unsafe extern "C" fn trueos_cabi_ui4_hull_text_runs_v1(
    window_id: u32,
    runs: *const TrueosUi4HullTextRunV1,
    run_count: usize,
) -> i32 {
    if run_count > MAX_RUNS || (run_count != 0 && runs.is_null()) {
        return ERROR_INVALID;
    }
    let records = if run_count == 0 {
        &[][..]
    } else {
        // SAFETY: the C ABI requires run_count readable input records. A guest
        // reads its own address space here; no raw pointers cross the VM call.
        unsafe { core::slice::from_raw_parts(runs, run_count) }
    };
    let mut payload = Vec::with_capacity(4 + run_count * WIRE_RUN_BYTES);
    payload.extend_from_slice(&(run_count as u32).to_le_bytes());
    let mut text_bytes = 0usize;
    for record in records {
        text_bytes = match text_bytes.checked_add(record.text_len) {
            Some(bytes) if bytes <= MAX_TEXT_BYTES => bytes,
            _ => return ERROR_INVALID,
        };
        if record.text_len > MAX_RUN_TEXT_BYTES
            || (record.text_len != 0 && record.text.is_null())
            || !valid_geometry(record.x, record.baseline_y, record.size_px, record.advance_px)
        {
            return ERROR_INVALID;
        }
        let text = if record.text_len == 0 {
            &[][..]
        } else {
            // SAFETY: caller supplies text_len readable bytes; these are copied
            // before the host processes the request.
            unsafe { core::slice::from_raw_parts(record.text, record.text_len) }
        };
        if core::str::from_utf8(text).is_err() {
            return ERROR_INVALID;
        }
        for value in [
            record.x,
            record.baseline_y,
            record.size_px,
            record.advance_px,
        ] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        payload.extend_from_slice(&record.rgba.to_le_bytes());
        payload.extend_from_slice(&(record.text_len as u32).to_le_bytes());
        payload.extend_from_slice(&record.column_offset.to_le_bytes());
        payload.extend_from_slice(text);
    }
    if payload.len() > trueos_vm::vmcall::PAYLOAD_CAP {
        return ERROR_INVALID;
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return guest_status(
            trueos_vm::vmcall::OP_BP_UI4_HULL_TEXT_RUNS_V1,
            window_id as u64,
            0,
            &payload,
        );
    }
    let Some(owner) = blueprint_owner() else {
        return ERROR_CONTEXT;
    };
    draw_wire(owner, window_id, &payload)
}

fn decode_wire(payload: &[u8]) -> Result<Vec<HullTextRun>, i32> {
    let header: [u8; 4] = payload.get(..4).ok_or(ERROR_INVALID)?.try_into().unwrap();
    let count = u32::from_le_bytes(header) as usize;
    if count > MAX_RUNS {
        return Err(ERROR_INVALID);
    }
    let mut offset = 4usize;
    let mut text_bytes = 0usize;
    let mut runs = Vec::with_capacity(count);
    for _ in 0..count {
        let record = payload
            .get(offset..offset + WIRE_RUN_BYTES)
            .ok_or(ERROR_INVALID)?;
        let word = |index: usize| u32::from_le_bytes(record[index..index + 4].try_into().unwrap());
        let (x, baseline_y, size_px, advance_px) = (
            f32::from_bits(word(0)),
            f32::from_bits(word(4)),
            f32::from_bits(word(8)),
            f32::from_bits(word(12)),
        );
        let length = word(20) as usize;
        text_bytes = text_bytes.checked_add(length).ok_or(ERROR_INVALID)?;
        if length > MAX_RUN_TEXT_BYTES
            || text_bytes > MAX_TEXT_BYTES
            || !valid_geometry(x, baseline_y, size_px, advance_px)
        {
            return Err(ERROR_INVALID);
        }
        offset += WIRE_RUN_BYTES;
        let end = offset.checked_add(length).ok_or(ERROR_INVALID)?;
        let text = core::str::from_utf8(payload.get(offset..end).ok_or(ERROR_INVALID)?)
            .map_err(|_| ERROR_INVALID)?;
        // Long natural-width runs are split without asking the Blueprint to
        // duplicate the service's physical-size quantization calculation.
        let x = x + hull_font_service::natural_advance_px(size_px) * word(24) as f32;
        runs.push(HullTextRun {
            text: String::from(text),
            x,
            baseline_y,
            size_px,
            advance_px,
            rgba: word(16),
        });
        offset = end;
    }
    if offset != payload.len() {
        return Err(ERROR_INVALID);
    }
    Ok(runs)
}

pub(crate) fn draw_wire(owner: WindowOwner, window_id: u32, payload: &[u8]) -> i32 {
    let overlay_started = crate::ui4::text_blit::stamp();
    let runs = match decode_wire(payload) {
        Ok(runs) => runs,
        Err(error) => return error,
    };
    let (lease, width, height) = {
        let mut surfaces = SURFACES.lock();
        let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
            return ERROR_NOT_FOUND;
        };
        if surface.render_target != surface.window.raw() {
            return ERROR_INVALID;
        }
        if surface.gpu_submission_unretired {
            return ERROR_BUSY;
        }
        let Some(lease) = surface.write_lease else {
            return ERROR_STATE;
        };
        (lease, surface.width, surface.height)
    };
    if runs.is_empty() {
        return 0;
    }
    let prepared = match hull_font_service::prepare_blocking(runs, width, height) {
        Ok(prepared) => prepared,
        Err(_) => return ERROR_UI4,
    };
    let mut surfaces = SURFACES.lock();
    let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
        return ERROR_NOT_FOUND;
    };
    if surface.write_lease != Some(lease) || surface.width != width || surface.height != height {
        return ERROR_STATE;
    }
    if surface.gpu_submission_unretired {
        return ERROR_BUSY;
    }
    let release = match (surface.pending_gpu_release, surface.pending_render_release) {
        (Some(release), None) => Some(super::super::FrameGpuRelease::Compute(release)),
        (None, Some(release)) => Some(super::super::FrameGpuRelease::ResidentScene(release)),
        (None, None) if surface.sprite_clear_rgba.is_none() => None,
        _ => return ERROR_STATE,
    };
    // The exact GPU allocation must be retired before CPU reads its background.
    // This transition validates its release proof instead of dropping a fence.
    if super::super::frame_pool::mark_frame_buffer_cpu_overlay(lease, release).is_err() {
        return ERROR_STATE;
    }
    surface.pending_gpu_release = None;
    surface.pending_render_release = None;
    surface.sprite_clear_rgba = None;
    let view = match super::writable_rgba_view(lease) {
        Ok(view) => view,
        Err(_) => return ERROR_UI4,
    };
    let region = match paint_flush_region(
        width,
        height,
        view.pitch,
        view.byte_len,
        prepared.paint_bounds(width, height),
    ) {
        Ok(region) => region,
        Err(error) => return error,
    };
    let span = region.as_ref().map(|region| {
        // SAFETY: paint_flush_region proved every row belongs to this allocation.
        crate::intel::DmaFlushRows::new(
            unsafe { view.virt.add(region.offset) },
            region.row_bytes,
            view.pitch as usize,
            region.rows,
        )
    });
    let spans = span.as_slice();
    let cache_sync_started = crate::ui4::text_blit::stamp();
    if !crate::intel::dma_flush_strided_row_spans(spans) {
        return ERROR_UI4;
    }
    let paint_started = crate::ui4::text_blit::stamp();
    // SAFETY: SURFACES retains this exact exclusive write lease throughout
    // blending. CPU cache invalidation follows the validated GPU release.
    let pixels = unsafe { core::slice::from_raw_parts_mut(view.virt, view.byte_len) };
    let result = prepared.paint(pixels, width, height, view.pitch);
    let paint_finished = crate::ui4::text_blit::stamp();
    let cache_sync_ok = crate::intel::dma_flush_strided_row_spans(spans);
    let overlay_finished = crate::ui4::text_blit::stamp();
    let cache_sync_ticks = paint_started
        .wrapping_sub(cache_sync_started)
        .saturating_add(overlay_finished.wrapping_sub(paint_finished));
    drop(surfaces);
    let status = if result.is_err() || !cache_sync_ok {
        ERROR_UI4
    } else {
        0
    };
    let cache_sync_bytes = region
        .as_ref()
        .map_or(0, |region| region.row_bytes * region.rows * 2);
    let cache_sync_rows = region.as_ref().map_or(0, |region| region.rows * 2);
    crate::log_important!(target: "hull-font";
        "hull-font: overlay owner={:?} window={} extent={}x{} frame_bytes={} overlay_us={} cache_sync_us={} paint_us={} cache_sync_bytes={} cache_sync_rows={} status={}\n",
        owner, window_id, width, height, view.byte_len,
        crate::ui4::text_blit::nanos(overlay_finished.wrapping_sub(overlay_started)) / 1000,
        crate::ui4::text_blit::nanos(cache_sync_ticks) / 1000,
        crate::ui4::text_blit::nanos(paint_finished.wrapping_sub(paint_started)) / 1000,
        cache_sync_bytes, cache_sync_rows, status);
    status
}
