//! Owned, frame-scoped cursor registration. Selection never uploads pixels.
use super::{
    ERROR_CONTEXT, ERROR_INVALID, ERROR_NOT_FOUND, ERROR_UI4, SURFACES, blueprint_owner,
    guest_status, surface_mut,
};
use v::bp_abi::TrueosUi4CursorImageV1;

const HEADER_BYTES: usize = 20;
const _: () = assert!(core::mem::size_of::<TrueosUi4CursorImageV1>() == HEADER_BYTES);

/// Copies straight-alpha, top-down RGBA8 into the owning frame's registry.
/// Registration does not select the cursor. Replacing an ID is atomic.
pub unsafe extern "C" fn trueos_cabi_ui4_scene_register_cursor_image_v1(
    window_id: u32,
    image: *const TrueosUi4CursorImageV1,
    rgba: *const u8,
    bytes: usize,
) -> i32 {
    if image.is_null() || rgba.is_null() {
        return ERROR_INVALID;
    }
    let image = unsafe { image.read_unaligned() };
    if !(1..=16).contains(&image.id)
        || !(1..=64).contains(&image.width)
        || !(1..=64).contains(&image.height)
        || image.hotspot_x >= image.width
        || image.hotspot_y >= image.height
        || bytes != image.width as usize * image.height as usize * 4
    {
        return ERROR_INVALID;
    }
    let pixels = unsafe { core::slice::from_raw_parts(rgba, bytes) };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let mut payload = alloc::vec::Vec::with_capacity(HEADER_BYTES + bytes);
        for value in [
            image.id,
            image.width,
            image.height,
            image.hotspot_x,
            image.hotspot_y,
        ] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        payload.extend_from_slice(pixels);
        return guest_status(
            trueos_vm::vmcall::OP_BP_UI4_SCENE_REGISTER_CURSOR_IMAGE_V1,
            window_id as u64,
            0,
            &payload,
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
    match crate::ui4::register_window_cursor_image(
        owner,
        window,
        image.id as u8,
        image.width,
        image.height,
        image.hotspot_x,
        image.hotspot_y,
        pixels,
    ) {
        Ok(()) => 0,
        Err(
            crate::ui4::CursorFrameError::NotFound | crate::ui4::CursorFrameError::ImageNotFound,
        ) => ERROR_NOT_FOUND,
        Err(crate::ui4::CursorFrameError::InvalidImage) => ERROR_INVALID,
        Err(_) => ERROR_UI4,
    }
}

/// Select a pre-registered frame-local ID; zero restores the default cursor.
pub extern "C" fn trueos_cabi_ui4_scene_select_cursor_image_v1(window_id: u32, id: u32) -> i32 {
    if id > 16 {
        return ERROR_INVALID;
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return guest_status(
            trueos_vm::vmcall::OP_BP_UI4_SCENE_SELECT_CURSOR_IMAGE_V1,
            window_id as u64,
            id as u64,
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
    match crate::ui4::select_window_cursor_image(owner, window, id as u8) {
        Ok(()) => 0,
        Err(
            crate::ui4::CursorFrameError::NotFound | crate::ui4::CursorFrameError::ImageNotFound,
        ) => ERROR_NOT_FOUND,
        Err(crate::ui4::CursorFrameError::InvalidImage) => ERROR_INVALID,
        Err(_) => ERROR_UI4,
    }
}
