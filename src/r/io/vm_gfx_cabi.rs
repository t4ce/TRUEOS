extern crate alloc;

use alloc::vec::Vec;

const VM_TEXTURE_GUEST_ID_LIMIT: u32 = 65_535;
const VM_TEXTURE_HOST_BASE: u32 = 50_000;
const VM_TEXTURE_HOST_STRIDE: u32 = 65_536;
const UI3_TEX_STATUS_UNKNOWN: i32 = 0;

#[derive(Clone, Copy)]
struct VmTextureMeta {
    vm_id: u8,
    tex_id: u32,
    width: u32,
    height: u32,
}

struct VmTextureUploadPending {
    vm_id: u8,
    guest_tex_id: u32,
    host_tex_id: u32,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    received: usize,
}

static VM_TEXTURE_META: spin::Mutex<Vec<VmTextureMeta>> = spin::Mutex::new(Vec::new());
static VM_TEXTURE_UPLOADS: spin::Mutex<Vec<VmTextureUploadPending>> = spin::Mutex::new(Vec::new());

#[inline]
fn vm_guest_texture_id_valid(tex_id: u32, op: &'static str) -> bool {
    if tex_id == 0 {
        return false;
    }
    if tex_id > VM_TEXTURE_GUEST_ID_LIMIT {
        crate::log!(
            "ui3-vm-gfx-cabi: reject vm texture id tex={} op={} max_guest={}\n",
            tex_id,
            op,
            VM_TEXTURE_GUEST_ID_LIMIT
        );
        return false;
    }
    true
}

#[inline]
fn get_u32(payload: &[u8], offset: usize) -> Option<u32> {
    payload
        .get(offset..offset + 4)
        .map(|bytes| u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

#[inline]
fn expected_rgba_len(width: u32, height: u32) -> Option<usize> {
    (width as usize)
        .checked_mul(height as usize)?
        .checked_mul(4)
}

#[inline]
pub fn host_texture_id_for_vm(vm_id: u8, tex_id: u32) -> u32 {
    if !vm_guest_texture_id_valid(tex_id, "host-texture-map") {
        return 0;
    }
    VM_TEXTURE_HOST_BASE
        .saturating_add((vm_id as u32).saturating_mul(VM_TEXTURE_HOST_STRIDE))
        .saturating_add(tex_id)
}

pub fn claim_vm_texture_id_for_vm(vm_id: u8, tex_id: u32, reason: &'static str) -> bool {
    if !vm_guest_texture_id_valid(tex_id, reason) {
        return false;
    }
    let mut meta = VM_TEXTURE_META.lock();
    if meta
        .iter()
        .any(|entry| entry.vm_id == vm_id && entry.tex_id == tex_id)
    {
        return true;
    }
    meta.push(VmTextureMeta {
        vm_id,
        tex_id,
        width: 0,
        height: 0,
    });
    true
}

pub fn record_vm_texture_dimensions_for_vm(vm_id: u8, tex_id: u32, width: u32, height: u32) {
    if width == 0 || height == 0 || !claim_vm_texture_id_for_vm(vm_id, tex_id, "vm-texture-meta") {
        return;
    }
    let mut meta = VM_TEXTURE_META.lock();
    if let Some(entry) = meta
        .iter_mut()
        .find(|entry| entry.vm_id == vm_id && entry.tex_id == tex_id)
    {
        entry.width = width;
        entry.height = height;
    }
}

pub fn handle_vm_texture_dimensions(vm_id: u8, tex_id: u32) -> Option<(u32, u32)> {
    if !vm_guest_texture_id_valid(tex_id, "vmcall-texture-dimensions") {
        return None;
    }
    VM_TEXTURE_META
        .lock()
        .iter()
        .find(|entry| {
            entry.vm_id == vm_id && entry.tex_id == tex_id && entry.width != 0 && entry.height != 0
        })
        .map(|entry| (entry.width, entry.height))
        .or_else(|| crate::ui3::ui3_img::image_dimensions(host_texture_id_for_vm(vm_id, tex_id)))
}

pub fn handle_vm_texture_status(vm_id: u8, tex_id: u32) -> i32 {
    if !vm_guest_texture_id_valid(tex_id, "vmcall-texture-status") {
        return UI3_TEX_STATUS_UNKNOWN;
    }
    crate::ui3::ui3_img::image_status(host_texture_id_for_vm(vm_id, tex_id))
}

pub fn host_texture_has_image(tex_id: u32) -> bool {
    crate::ui3::ui3_img::image_dimensions(tex_id).is_some()
}

pub fn queue_texture_rgba_image_upload_owned(
    tex_id: u32,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    _sample_kind: u32,
    _reason: &'static str,
) -> bool {
    crate::ui3::ui3_img::store_rgba_image(tex_id, width, height, rgba) == 0
}

pub fn queue_host_texture_rgba_image_upload_owned(
    tex_id: u32,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    sample_kind: u32,
    reason: &'static str,
) -> bool {
    queue_texture_rgba_image_upload_owned(tex_id, width, height, rgba, sample_kind, reason)
}

pub fn handle_vm_texture_upload_begin(vm_id: u8, payload: &[u8]) -> i32 {
    const HEADER_LEN: usize = 40;
    if payload.len() < HEADER_LEN {
        return -1;
    }
    let guest_tex_id = get_u32(payload, 0).unwrap_or(0);
    let width = get_u32(payload, 4).unwrap_or(0);
    let height = get_u32(payload, 8).unwrap_or(0);
    let region_flag = get_u32(payload, 12).unwrap_or(0);
    let total_len = get_u32(payload, 36).unwrap_or(0) as usize;
    if region_flag != 0 {
        return -6;
    }
    if guest_tex_id == 0 || width == 0 || height == 0 {
        return -3;
    }
    if !claim_vm_texture_id_for_vm(vm_id, guest_tex_id, "vm-texture-upload-begin") {
        return -4;
    }
    let host_tex_id = host_texture_id_for_vm(vm_id, guest_tex_id);
    let Some(expected) = expected_rgba_len(width, height) else {
        return -6;
    };
    if total_len < expected {
        return -7;
    }

    let mut uploads = VM_TEXTURE_UPLOADS.lock();
    uploads.retain(|upload| upload.vm_id != vm_id);
    let mut rgba = Vec::new();
    if rgba.try_reserve_exact(expected).is_err() {
        return -8;
    }
    rgba.resize(expected, 0);
    uploads.push(VmTextureUploadPending {
        vm_id,
        guest_tex_id,
        host_tex_id,
        width,
        height,
        rgba,
        received: 0,
    });
    0
}

pub fn handle_vm_texture_upload_chunk(vm_id: u8, offset: usize, payload: &[u8]) -> i32 {
    let mut uploads = VM_TEXTURE_UPLOADS.lock();
    let Some(upload) = uploads.iter_mut().find(|upload| upload.vm_id == vm_id) else {
        return -1;
    };
    if offset != upload.received {
        return -2;
    }
    let end = offset.saturating_add(payload.len());
    if end > upload.rgba.len() {
        return -3;
    }
    upload.rgba[offset..end].copy_from_slice(payload);
    upload.received = end;
    0
}

pub fn handle_vm_texture_upload_finish(vm_id: u8) -> i32 {
    let pending = {
        let mut uploads = VM_TEXTURE_UPLOADS.lock();
        let Some(idx) = uploads.iter().position(|upload| upload.vm_id == vm_id) else {
            return -1;
        };
        uploads.swap_remove(idx)
    };
    if pending.received != pending.rgba.len() {
        return -2;
    }
    record_vm_texture_dimensions_for_vm(
        pending.vm_id,
        pending.guest_tex_id,
        pending.width,
        pending.height,
    );
    if crate::ui3::ui3_img::store_rgba_image(
        pending.host_tex_id,
        pending.width,
        pending.height,
        pending.rgba,
    ) == 0
    {
        0
    } else {
        -9
    }
}

pub fn handle_vm_queue_render_rgb(
    _vm_id: u8,
    _tex_id: u32,
    _clear_rgb: u32,
    _repaint_window_id: u32,
    _vtx: &[u8],
) -> bool {
    false
}

pub fn handle_vm_queue_render_tex(
    _vm_id: u8,
    _target_tex_id: u32,
    _source_tex_id: u32,
    _clear_rgb: u32,
    _repaint_window_id: u32,
    _vtx: &[u8],
) -> bool {
    false
}

pub fn handle_vm_queue_render_mandelbrot(
    _vm_id: u8,
    _tex_id: u32,
    _ticks: u64,
    _tick_hz: u64,
    _repaint_window_id: u32,
) -> bool {
    false
}

pub fn handle_vm_render_upload_begin(_vm_id: u8, _payload: &[u8]) -> i32 {
    -90
}

pub fn handle_vm_render_upload_chunk(_vm_id: u8, _offset: usize, _payload: &[u8]) -> i32 {
    -91
}

pub fn handle_vm_render_upload_finish(_vm_id: u8) -> bool {
    false
}

pub fn handle_vm_gfx_frame_begin(_vm_id: u8, _clear_rgb: u32, _flags: u32) -> i32 {
    -90
}

pub fn handle_vm_gfx_frame_set_render_target(_vm_id: u8, _tex_id: u32) -> i32 {
    -90
}

pub fn handle_vm_gfx_frame_state(_vm_id: u8, _kind: u32, _payload: &[u8]) -> i32 {
    -90
}

pub fn handle_vm_gfx_frame_draw_begin(_vm_id: u8, _payload: &[u8]) -> i32 {
    -90
}

pub fn handle_vm_gfx_frame_draw_chunk(_vm_id: u8, _offset: usize, _payload: &[u8]) -> i32 {
    -90
}

pub fn handle_vm_gfx_frame_draw_finish(_vm_id: u8) -> i32 {
    -90
}

pub fn handle_vm_gfx_frame_end(_vm_id: u8) -> i32 {
    -90
}
