//! Experimental Odin-generated compute attached to the caller's existing UI4 lease.

use super::{
    ERROR_BUSY, ERROR_CONTEXT, ERROR_INVALID, ERROR_NOT_FOUND, ERROR_STATE, ERROR_UI4,
    SURFACES, WindowOwner, blueprint_owner, guest_status, surface_mut, gpgpu_rgba_surface,
};
use crate::intel::gpgpu::{
    GeneratedPixelProgram, GeneratedPixelParamsV1, create_generated_pixel_program,
    generated_pixel_rgba8_surface_full,
};
use alloc::vec::Vec;

const MAX_CODE_BYTES: usize = 24 * 1024;
const MAX_CHUNK_BYTES: usize = 2048;
const ERROR_UNSUPPORTED: i32 = -95;

#[derive(Default)]
pub(super) struct NativeComputeState {
    upload: Option<CodeUpload>,
    program: Option<GeneratedPixelProgram>,
}

struct CodeUpload {
    total: usize,
    bytes: Vec<u8>,
}

pub unsafe extern "C" fn trueos_cabi_ui4_scene_native_compute_upload_v1(
    window_id: u32, offset: u32, code_len: u32, bytes: *const u8, len: usize,
) -> i32 {
    if !cfg!(feature = "odin-native-gpgpu") { return ERROR_UNSUPPORTED; }
    if bytes.is_null() || len == 0 || len > MAX_CHUNK_BYTES { return ERROR_INVALID; }
    let data = unsafe { core::slice::from_raw_parts(bytes, len) };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return guest_status(
            trueos_vm::vmcall::OP_BP_UI4_NATIVE_COMPUTE_UPLOAD_V1,
            (u64::from(code_len) << 32) | u64::from(window_id), u64::from(offset), data,
        );
    }
    let Some(owner) = blueprint_owner() else { return ERROR_CONTEXT; };
    upload_chunk(owner, window_id, offset as usize, code_len as usize, data)
}

pub(crate) fn upload_chunk(
    owner: WindowOwner, window_id: u32, offset: usize, total: usize, bytes: &[u8],
) -> i32 {
    if !cfg!(feature = "odin-native-gpgpu") { return ERROR_UNSUPPORTED; }
    if total == 0 || total > MAX_CODE_BYTES || total % 16 != 0
        || bytes.is_empty() || bytes.len() > MAX_CHUNK_BYTES
    { return ERROR_INVALID; }
    let complete = {
        let mut surfaces = SURFACES.lock();
        let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else { return ERROR_NOT_FOUND; };
        if surface.visual_cadence.is_none() { return ERROR_INVALID; }
        if surface.gpu_submission_unretired { return ERROR_BUSY; }
        if offset == 0 {
            let mut staging = Vec::new();
            if staging.try_reserve_exact(total).is_err() { return ERROR_UI4; }
            surface.native_compute.upload = Some(CodeUpload { total, bytes: staging });
        }
        let Some(upload) = surface.native_compute.upload.as_mut() else { return ERROR_STATE; };
        if upload.total != total || offset != upload.bytes.len()
            || bytes.len() > total.saturating_sub(offset)
        {
            surface.native_compute.upload = None;
            return ERROR_INVALID;
        }
        upload.bytes.extend_from_slice(bytes);
        if upload.bytes.len() != total { return 0; }
        surface.native_compute.upload.take().unwrap()
    };
    let program = match create_generated_pixel_program(&complete.bytes) {
        Ok(program) => program,
        Err(rc) => return rc,
    };
    let mut surfaces = SURFACES.lock();
    let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else { return ERROR_NOT_FOUND; };
    if surface.gpu_submission_unretired { return ERROR_BUSY; }
    crate::log_info!(target: "gpgpu";
        "odin-native: program registered owner={:?} window={} bytes={} admission=runtime-generated\n",
        owner, window_id, program.code_len());
    surface.native_compute.program = Some(program);
    0
}

pub unsafe extern "C" fn trueos_cabi_ui4_scene_native_compute_render_v1(
    window_id: u32, origin_x: f32, origin_y: f32, step_x: f32, step_y: f32,
) -> i32 {
    if !cfg!(feature = "odin-native-gpgpu") { return ERROR_UNSUPPORTED; }
    let mut payload = [0u8; 16];
    for (dst, value) in payload.chunks_exact_mut(4).zip([origin_x, origin_y, step_x, step_y]) {
        dst.copy_from_slice(&value.to_le_bytes());
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return guest_status(
            trueos_vm::vmcall::OP_BP_UI4_NATIVE_COMPUTE_RENDER_V1,
            u64::from(window_id), 0, &payload,
        );
    }
    let Some(owner) = blueprint_owner() else { return ERROR_CONTEXT; };
    render_wire(owner, window_id, &payload)
}

pub(crate) fn render_wire(owner: WindowOwner, window_id: u32, payload: &[u8]) -> i32 {
    if !cfg!(feature = "odin-native-gpgpu") { return ERROR_UNSUPPORTED; }
    if payload.len() != 16 { return ERROR_INVALID; }
    let mut values = [0f32; 4];
    for (dst, src) in values.iter_mut().zip(payload.chunks_exact(4)) {
        *dst = f32::from_le_bytes(src.try_into().unwrap());
    }
    if values.iter().any(|v| !v.is_finite()) || values[2] <= 0.0 || values[3] <= 0.0 {
        return ERROR_INVALID;
    }
    let params = GeneratedPixelParamsV1 {
        origin_x: values[0], origin_y: values[1], step_x: values[2], step_y: values[3],
    };
    let (lease, destination, program) = {
        let mut surfaces = SURFACES.lock();
        let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else { return ERROR_NOT_FOUND; };
        if surface.gpu_submission_unretired { return ERROR_BUSY; }
        let Some(lease) = surface.write_lease else { return ERROR_STATE; };
        if surface.pending_gpu_release.is_some() || surface.pending_render_release.is_some() {
            return ERROR_STATE;
        }
        let Ok(destination) = gpgpu_rgba_surface(lease) else { return ERROR_UI4; };
        let Some(program) = surface.native_compute.program.take() else { return ERROR_STATE; };
        surface.gpu_submission_unretired = true;
        (lease, destination, program)
    };
    let result = generated_pixel_rgba8_surface_full(&program, destination, params);
    let mut surfaces = SURFACES.lock();
    let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else { return ERROR_NOT_FOUND; };
    surface.native_compute.program = Some(program);
    if surface.write_lease != Some(lease) { return ERROR_STATE; }
    if result.ok || !result.submitted { surface.gpu_submission_unretired = false; }
    if result.ok {
        let Some(release) = result.release else { return ERROR_UI4; };
        surface.pending_gpu_release = Some(release);
        return 0;
    }
    if result.submitted { ERROR_UI4 } else { ERROR_BUSY }
}
