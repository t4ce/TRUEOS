//! Experimental Odin-generated compute attached to the caller's existing UI4 lease.

use super::{
    ERROR_BUSY, ERROR_CONTEXT, ERROR_INVALID, ERROR_NOT_FOUND, ERROR_STATE, ERROR_UI4, SURFACES,
    WindowOwner, blueprint_owner, gpgpu_rgba_surface, guest_status, surface_mut,
};
use crate::intel::gpgpu::{
    GeneratedPixelParamsV1, GeneratedPixelProgram, NativeKernelProgram,
    create_generated_pixel_program, create_native_grid_pixel_program,
    create_native_kernel_program, dispatch_native_kernel,
    generated_pixel_rgba8_surface_full, generated_field_rgba8_surface_full,
    create_native_field_pixel_program, GENERATED_FIELD_BYTES,
};
use alloc::vec::Vec;

const MAX_CODE_BYTES: usize = 24 * 1024;
const MAX_CHUNK_BYTES: usize = 2048;
const ERROR_UNSUPPORTED: i32 = -95;

#[derive(Default)]
pub(super) struct NativeComputeState {
    upload: Option<CodeUpload>,
    program: Option<GeneratedPixelProgram>,
    kernel: Option<NativeKernelProgram>,
    field_program: Option<GeneratedPixelProgram>,
    field_upload: Option<CodeUpload>,
    fields: Option<Vec<u8>>,
}

struct CodeUpload {
    total: usize,
    bytes: Vec<u8>,
}

pub unsafe extern "C" fn trueos_cabi_ui4_scene_native_compute_upload_v1(
    window_id: u32,
    offset: u32,
    code_len: u32,
    bytes: *const u8,
    len: usize,
) -> i32 {
    if !cfg!(feature = "odin-native-gpgpu") {
        return ERROR_UNSUPPORTED;
    }
    if bytes.is_null() || len == 0 || len > MAX_CHUNK_BYTES {
        return ERROR_INVALID;
    }
    let data = unsafe { core::slice::from_raw_parts(bytes, len) };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return guest_status(
            trueos_vm::vmcall::OP_BP_UI4_NATIVE_COMPUTE_UPLOAD_V1,
            (u64::from(code_len) << 32) | u64::from(window_id),
            u64::from(offset),
            data,
        );
    }
    let Some(owner) = blueprint_owner() else {
        return ERROR_CONTEXT;
    };
    upload_chunk(owner, window_id, offset as usize, code_len as usize, data)
}

pub(crate) fn upload_chunk(
    owner: WindowOwner,
    window_id: u32,
    offset: usize,
    total: usize,
    bytes: &[u8],
) -> i32 {
    if !cfg!(feature = "odin-native-gpgpu") {
        return ERROR_UNSUPPORTED;
    }
    if total == 0
        || total > MAX_CODE_BYTES
        || total % 16 != 0
        || bytes.is_empty()
        || bytes.len() > MAX_CHUNK_BYTES
    {
        return ERROR_INVALID;
    }
    let complete = {
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
        if offset == 0 {
            surface.native_compute.program = None;
            surface.native_compute.kernel = None;
            surface.native_compute.field_program = None;
            let mut staging = Vec::new();
            if staging.try_reserve_exact(total).is_err() {
                return ERROR_UI4;
            }
            surface.native_compute.upload = Some(CodeUpload {
                total,
                bytes: staging,
            });
        }
        let Some(upload) = surface.native_compute.upload.as_mut() else {
            return ERROR_STATE;
        };
        if upload.total != total
            || offset != upload.bytes.len()
            || bytes.len() > total.saturating_sub(offset)
        {
            surface.native_compute.upload = None;
            return ERROR_INVALID;
        }
        upload.bytes.extend_from_slice(bytes);
        if upload.bytes.len() != total {
            return 0;
        }
        surface.native_compute.upload.take().unwrap()
    };
    let (pixel, kernel) = match create_generated_pixel_program(&complete.bytes) {
        Ok(program) => (Some(program), None),
        Err(_) => match create_native_kernel_program(&complete.bytes) {
            Ok(program) => (create_native_grid_pixel_program(&complete.bytes).ok(), Some(program)),
            Err(rc) => return rc,
        },
    };
    let mut surfaces = SURFACES.lock();
    let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
        return ERROR_NOT_FOUND;
    };
    if surface.gpu_submission_unretired {
        return ERROR_BUSY;
    }
    crate::log_info!(target: "gpgpu";
        "odin-native: program registered owner={:?} window={} bytes={} admission=runtime-generated\n",
        owner, window_id, complete.bytes.len());
    surface.native_compute.field_program = create_native_field_pixel_program(&complete.bytes).ok();
    surface.native_compute.program = pixel;
    surface.native_compute.kernel = kernel;
    0
}

pub unsafe extern "C" fn trueos_cabi_ui4_scene_native_compute_render_v1(
    window_id: u32,
    origin_x: f32,
    origin_y: f32,
    step_x: f32,
    step_y: f32,
) -> i32 {
    if !cfg!(feature = "odin-native-gpgpu") {
        return ERROR_UNSUPPORTED;
    }
    let mut payload = [0u8; 16];
    for (dst, value) in payload
        .chunks_exact_mut(4)
        .zip([origin_x, origin_y, step_x, step_y])
    {
        dst.copy_from_slice(&value.to_le_bytes());
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return guest_status(
            trueos_vm::vmcall::OP_BP_UI4_NATIVE_COMPUTE_RENDER_V1,
            u64::from(window_id),
            0,
            &payload,
        );
    }
    let Some(owner) = blueprint_owner() else {
        return ERROR_CONTEXT;
    };
    render_wire(owner, window_id, &payload)
}

pub(crate) fn render_wire(owner: WindowOwner, window_id: u32, payload: &[u8]) -> i32 {
    render_params_wire(owner, window_id, payload, 16)
}

/// V2 is ten packed f32 values: origin.xy, step.xy, then six scalar uniforms.
pub unsafe extern "C" fn trueos_cabi_ui4_scene_native_compute_render_v2(
    window_id: u32, params: *const f32, len: usize,
) -> i32 {
    if !cfg!(feature = "odin-native-gpgpu") { return ERROR_UNSUPPORTED; }
    if params.is_null() || params.addr() % 4 != 0 || len != 40 { return ERROR_INVALID; }
    let values = unsafe { core::slice::from_raw_parts(params, 10) };
    let mut payload = [0u8; 40];
    for (dst, value) in payload.chunks_exact_mut(4).zip(values) {
        dst.copy_from_slice(&value.to_le_bytes());
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return guest_status(trueos_vm::vmcall::OP_BP_UI4_NATIVE_COMPUTE_RENDER_V2,
            u64::from(window_id), 0, &payload);
    }
    let Some(owner) = blueprint_owner() else { return ERROR_CONTEXT; };
    render_uniforms_wire(owner, window_id, &payload)
}

pub(crate) fn render_uniforms_wire(owner: WindowOwner, window_id: u32, payload: &[u8]) -> i32 {
    render_params_wire(owner, window_id, payload, 40)
}

fn render_params_wire(owner: WindowOwner, window_id: u32, payload: &[u8], expected: usize) -> i32 {
    if !cfg!(feature = "odin-native-gpgpu") {
        return ERROR_UNSUPPORTED;
    }
    if payload.len() != expected {
        return ERROR_INVALID;
    }
    let mut values = [0f32; 10];
    for (dst, src) in values.iter_mut().zip(payload.chunks_exact(4)) {
        *dst = f32::from_le_bytes(src.try_into().unwrap());
    }
    if values.iter().any(|v| !v.is_finite()) || values[2] <= 0.0 || values[3] <= 0.0 {
        return ERROR_INVALID;
    }
    let params = GeneratedPixelParamsV1 {
        origin_x: values[0],
        origin_y: values[1],
        step_x: values[2],
        step_y: values[3],
        uniforms: values[4..].try_into().unwrap(),
    };
    let (lease, destination, program) = {
        let mut surfaces = SURFACES.lock();
        let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
            return ERROR_NOT_FOUND;
        };
        if surface.gpu_submission_unretired {
            return ERROR_BUSY;
        }
        let Some(lease) = surface.write_lease else {
            return ERROR_STATE;
        };
        if surface.pending_gpu_release.is_some() || surface.pending_render_release.is_some() {
            return ERROR_STATE;
        }
        let Ok(destination) = gpgpu_rgba_surface(lease) else {
            return ERROR_UI4;
        };
        let Some(program) = surface.native_compute.program.take() else {
            return ERROR_STATE;
        };
        surface.gpu_submission_unretired = true;
        (lease, destination, program)
    };
    let result = generated_pixel_rgba8_surface_full(&program, destination, params);
    let mut surfaces = SURFACES.lock();
    let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
        return ERROR_NOT_FOUND;
    };
    surface.native_compute.program = Some(program);
    if surface.write_lease != Some(lease) {
        return ERROR_STATE;
    }
    if result.ok || !result.submitted {
        surface.gpu_submission_unretired = false;
    }
    if result.ok {
        let Some(release) = result.release else {
            return ERROR_UI4;
        };
        surface.pending_gpu_release = Some(release);
        return 0;
    }
    if result.submitted {
        ERROR_UI4
    } else {
        ERROR_BUSY
    }
}

/// Dispatch the uploaded complete SIMD8 kernel once; inputs are 1..=8 GRFs,
/// each containing eight raw dword lanes. The result is one eight-lane GRF.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui4_scene_native_compute_dispatch_v1(
    window_id: u32,
    inputs: *const u32,
    registers: u32,
    out: *mut u32,
) -> i32 {
    if !cfg!(feature = "odin-native-gpgpu") {
        return ERROR_UNSUPPORTED;
    }
    if !(1..=8).contains(&registers)
        || inputs.is_null()
        || out.is_null()
        || inputs.addr() % 4 != 0
        || out.addr() % 4 != 0
    {
        return ERROR_INVALID;
    }
    let input = unsafe { core::slice::from_raw_parts(inputs, registers as usize * 8) };
    let mut payload = [0u8; 260];
    payload[..4].copy_from_slice(&registers.to_le_bytes());
    for (i, bits) in input.iter().enumerate() {
        payload[4 + 4 * i..8 + 4 * i].copy_from_slice(&bits.to_le_bytes());
    }
    let result = if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let mut reply = [0u8; 32];
        let (status, rc) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_UI4_NATIVE_COMPUTE_DISPATCH_V1,
            u64::from(window_id),
            0,
            &payload[..4 + input.len() * 4],
            &mut reply,
        );
        if status != trueos_vm::vmcall::STATUS_OK {
            Err(ERROR_UI4)
        } else if rc as i64 != 0 {
            Err(rc as i64 as i32)
        } else {
            Ok(core::array::from_fn(|i| {
                u32::from_le_bytes(reply[4 * i..4 * i + 4].try_into().unwrap())
            }))
        }
    } else {
        let Some(owner) = blueprint_owner() else {
            return ERROR_CONTEXT;
        };
        dispatch_wire(owner, window_id, &payload[..4 + input.len() * 4])
    };
    match result {
        Ok(lanes) => {
            unsafe {
                core::ptr::copy_nonoverlapping(lanes.as_ptr(), out, 8);
            }
            0
        }
        Err(rc) => rc,
    }
}

pub(crate) fn dispatch_wire(
    owner: WindowOwner,
    window_id: u32,
    payload: &[u8],
) -> Result<[u32; 8], i32> {
    if !cfg!(feature = "odin-native-gpgpu") {
        return Err(ERROR_UNSUPPORTED);
    }
    if payload.len() < 4 {
        return Err(ERROR_INVALID);
    }
    let registers = u32::from_le_bytes(payload[..4].try_into().unwrap()) as usize;
    if !(1..=8).contains(&registers) || payload.len() != 4 + 32 * registers {
        return Err(ERROR_INVALID);
    }
    let mut inputs = [[0u32; 8]; 8];
    for (i, bytes) in payload[4..].chunks_exact(4).enumerate() {
        inputs[i / 8][i % 8] = u32::from_le_bytes(bytes.try_into().unwrap());
    }
    let program = {
        let mut surfaces = SURFACES.lock();
        let surface = surface_mut(&mut surfaces, owner, window_id).ok_or(ERROR_NOT_FOUND)?;
        if surface.gpu_submission_unretired {
            return Err(ERROR_BUSY);
        }
        let program = surface.native_compute.kernel.take().ok_or(ERROR_STATE)?;
        surface.gpu_submission_unretired = true;
        program
    };
    let digest = program.digest();
    let bytes = program.code_len();
    let result = dispatch_native_kernel(&program, &inputs[..registers]);
    let mut surfaces = SURFACES.lock();
    let surface = surface_mut(&mut surfaces, owner, window_id).ok_or(ERROR_NOT_FOUND)?;
    surface.native_compute.kernel = Some(program);
    surface.gpu_submission_unretired = matches!(result, Err((_, true)));
    crate::log_info!(target: "gpgpu";
        "odin-native: dispatch owner={:?} window={} bytes={} sha256={:02x?} simd=8 registers={} status={}\n",
        owner, window_id, bytes, digest, registers, result.as_ref().err().map_or(0, |e| e.0));
    result.map_err(|(rc, _)| rc)
}

/// Six planar 192x144 finite f32 fields, consumed as SIMD8 r1..r6 by uploaded code.
pub unsafe extern "C" fn trueos_cabi_ui4_scene_native_compute_fields_upload_v1(
    window_id: u32, offset: u32, total: u32, bytes: *const u8, len: usize,
) -> i32 {
    if !cfg!(feature = "odin-native-gpgpu") { return ERROR_UNSUPPORTED; }
    if bytes.is_null() || len == 0 || len > MAX_CHUNK_BYTES { return ERROR_INVALID; }
    let data = unsafe { core::slice::from_raw_parts(bytes, len) };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return guest_status(trueos_vm::vmcall::OP_BP_UI4_NATIVE_COMPUTE_FIELDS_UPLOAD_V1,
            (u64::from(total) << 32) | u64::from(window_id), u64::from(offset), data);
    }
    let Some(owner) = blueprint_owner() else { return ERROR_CONTEXT; };
    fields_upload_chunk(owner, window_id, offset as usize, total as usize, data)
}

pub(crate) fn fields_upload_chunk(
    owner: WindowOwner, window_id: u32, offset: usize, total: usize, data: &[u8],
) -> i32 {
    if !cfg!(feature = "odin-native-gpgpu") { return ERROR_UNSUPPORTED; }
    if total != GENERATED_FIELD_BYTES || data.is_empty() || data.len() > MAX_CHUNK_BYTES
        || offset % 4 != 0 || data.len() % 4 != 0
        || data.chunks_exact(4).any(|v| !f32::from_le_bytes(v.try_into().unwrap()).is_finite())
    { return ERROR_INVALID; }
    let mut surfaces = SURFACES.lock();
    let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else { return ERROR_NOT_FOUND; };
    if surface.visual_cadence.is_none() { return ERROR_INVALID; }
    if surface.gpu_submission_unretired { return ERROR_BUSY; }
    let state = &mut surface.native_compute;
    if offset == 0 {
        state.fields = None;
        let mut bytes = Vec::new();
        if bytes.try_reserve_exact(total).is_err() { return ERROR_UI4; }
        state.field_upload = Some(CodeUpload { total, bytes });
    }
    let Some(upload) = state.field_upload.as_mut() else { return ERROR_STATE; };
    if upload.total != total || offset != upload.bytes.len() || data.len() > total.saturating_sub(offset) {
        state.field_upload = None;
        return ERROR_INVALID;
    }
    upload.bytes.extend_from_slice(data);
    if upload.bytes.len() == total {
        state.fields = Some(state.field_upload.take().unwrap().bytes);
    }
    0
}

pub unsafe extern "C" fn trueos_cabi_ui4_scene_native_compute_fields_render_v1(window_id: u32) -> i32 {
    if !cfg!(feature = "odin-native-gpgpu") { return ERROR_UNSUPPORTED; }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return guest_status(trueos_vm::vmcall::OP_BP_UI4_NATIVE_COMPUTE_FIELDS_RENDER_V1,
            u64::from(window_id), 0, &[]);
    }
    let Some(owner) = blueprint_owner() else { return ERROR_CONTEXT; };
    fields_render(owner, window_id)
}

pub(crate) fn fields_render(owner: WindowOwner, window_id: u32) -> i32 {
    if !cfg!(feature = "odin-native-gpgpu") { return ERROR_UNSUPPORTED; }
    let (lease, destination, program, fields) = {
        let mut surfaces = SURFACES.lock();
        let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else { return ERROR_NOT_FOUND; };
        if surface.gpu_submission_unretired { return ERROR_BUSY; }
        let Some(lease) = surface.write_lease else { return ERROR_STATE; };
        if surface.pending_gpu_release.is_some() || surface.pending_render_release.is_some() { return ERROR_STATE; }
        let Ok(destination) = gpgpu_rgba_surface(lease) else { return ERROR_UI4; };
        if surface.native_compute.field_program.is_none() || surface.native_compute.fields.is_none() { return ERROR_STATE; }
        let program = surface.native_compute.field_program.take().unwrap();
        let fields = surface.native_compute.fields.take().unwrap();
        surface.gpu_submission_unretired = true;
        (lease, destination, program, fields)
    };
    let result = generated_field_rgba8_surface_full(&program, destination, &fields);
    let mut surfaces = SURFACES.lock();
    let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else { return ERROR_NOT_FOUND; };
    surface.native_compute.field_program = Some(program);
    surface.native_compute.fields = Some(fields);
    if surface.write_lease != Some(lease) { return ERROR_STATE; }
    if result.ok || !result.submitted { surface.gpu_submission_unretired = false; }
    if result.ok {
        let Some(release) = result.release else { return ERROR_UI4; };
        surface.pending_gpu_release = Some(release);
        0
    } else if result.submitted { ERROR_UI4 } else { ERROR_BUSY }
}
