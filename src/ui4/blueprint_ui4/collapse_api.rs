//! Collapse registration shares the existing bounded, chunked image transport.
use super::*;
// Collapse uses the ordinary upload transport. Keep its transient ID in the
// allowed low-bit namespace used by ordinary Blueprint sprite uploads.
const UPLOAD_ID: u32 = 0x7FFF_FFFF;

/// Copy a straight-alpha RGBA8 icon, enabling kernel-owned collapse/restore.
/// Zero extents, images larger than 256x256, and incomplete uploads are rejected.
pub unsafe extern "C" fn trueos_cabi_ui4_scene_enable_collapse_v1(
    window_id: u32,
    width: u32,
    height: u32,
    rgba: *const u8,
    bytes: usize,
) -> i32 {
    if !(1..=crate::ui4::MAX_COLLAPSE_IMAGE_SIZE).contains(&width)
        || !(1..=crate::ui4::MAX_COLLAPSE_IMAGE_SIZE).contains(&height)
        || rgba.is_null()
        || bytes != width as usize * height as usize * 4
    {
        return ERROR_INVALID;
    }
    let pixels = unsafe { core::slice::from_raw_parts(rgba, bytes) };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let mut extent = [0u8; 8];
        extent[..4].copy_from_slice(&width.to_le_bytes());
        extent[4..].copy_from_slice(&height.to_le_bytes());
        let rc = guest_status(
            trueos_vm::vmcall::OP_BP_UI4_SCENE_SPRITE_UPLOAD_BEGIN,
            window_id as u64,
            UPLOAD_ID as u64,
            &extent,
        );
        if rc != 0 {
            return rc;
        }
        let chunk_size = trueos_vm::vmcall::PAYLOAD_CAP & !3;
        for (index, chunk) in pixels.chunks(chunk_size).enumerate() {
            let rc = guest_status(
                trueos_vm::vmcall::OP_BP_UI4_SCENE_SPRITE_UPLOAD_CHUNK,
                window_id as u64,
                ((UPLOAD_ID as u64) << 32) | (index * chunk_size) as u64,
                chunk,
            );
            if rc != 0 {
                return rc;
            }
        }
        return guest_status(
            trueos_vm::vmcall::OP_BP_UI4_SCENE_ENABLE_COLLAPSE_V1,
            window_id as u64,
            0,
            &[],
        );
    }
    let Some(owner) = blueprint_owner() else {
        return ERROR_CONTEXT;
    };
    let rc = begin_sprite_rgba8_upload(owner, window_id, UPLOAD_ID, width, height);
    if rc != 0 {
        return rc;
    }
    let rc = write_sprite_rgba8_upload_chunk(owner, window_id, UPLOAD_ID, 0, pixels);
    if rc != 0 {
        return rc;
    }
    finish(owner, window_id)
}

pub(crate) fn finish(owner: WindowOwner, window_id: u32) -> i32 {
    let (window, upload) = {
        let mut surfaces = SURFACES.lock();
        let Some(surface) = surface_mut(&mut surfaces, owner, window_id) else {
            return ERROR_NOT_FOUND;
        };
        if surface.render_target != surface.window.raw() {
            return ERROR_INVALID;
        }
        (surface.window, surface.sprite_upload.take())
    };
    let Some(upload) = upload else {
        return ERROR_STATE;
    };
    let source = upload.owned.surface;
    let result = if upload.sprite_id != UPLOAD_ID
        || upload.written != upload.packed_len
        || source.width > crate::ui4::MAX_COLLAPSE_IMAGE_SIZE
        || source.height > crate::ui4::MAX_COLLAPSE_IMAGE_SIZE
    {
        ERROR_INVALID
    } else {
        let pixels = unsafe { core::slice::from_raw_parts(upload.owned.virt, upload.owned.bytes) };
        crate::ui4::enable_window_collapse(
            owner,
            window,
            source.width,
            source.height,
            source.pitch_bytes as usize,
            pixels,
        )
        .map(|()| 0)
        .unwrap_or_else(|error| match error {
            crate::ui4::CollapseError::InvalidImage => ERROR_INVALID,
            crate::ui4::CollapseError::NotFound => ERROR_NOT_FOUND,
            crate::ui4::CollapseError::Busy => ERROR_BUSY,
            crate::ui4::CollapseError::Ui4 => ERROR_UI4,
        })
    };
    destroy_rgba8_surface(upload.owned);
    result
}
