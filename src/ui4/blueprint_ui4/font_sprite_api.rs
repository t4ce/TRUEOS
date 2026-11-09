//! Window-owned glyph sprites sampled from the existing shared Microfont atlas.
//! No font rasterizer or mask bytes cross the Blueprint boundary.
use super::{
    ERROR_BUSY, ERROR_CONTEXT, ERROR_INVALID, ERROR_NOT_FOUND, ERROR_UI4, SURFACES, WindowOwner,
    blueprint_owner, hull_image_api, surface_mut,
};
use alloc::vec;
use v::bp_abi::TrueosUi4FontSpriteStatusV1;

pub(super) struct Sprite {
    key: [u32; 4],
    id: u32,
    width: u32,
    height: u32,
}

pub(crate) fn request(owner: WindowOwner, window: u32, bytes: &[u8]) -> Result<u64, i32> {
    if bytes.len() != 16 {
        return Err(ERROR_INVALID);
    }
    let key: [u32; 4] =
        core::array::from_fn(|i| u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap()));
    let [font, scalar, pixels, color] = key;
    let height = f32::from_bits(pixels);
    let character = char::from_u32(scalar).ok_or(ERROR_INVALID)?;
    if font != 1 || !height.is_finite() || !(11.0..=256.0).contains(&height) {
        return Err(ERROR_INVALID);
    }
    let WindowOwner::Vm(vm) = owner else {
        return Err(ERROR_CONTEXT);
    };
    let incarnation = hull_image_api::authenticate_window(u32::from(vm), window)?;
    {
        let mut surfaces = SURFACES.try_lock().ok_or(ERROR_BUSY)?;
        let surface = surface_mut(&mut surfaces, owner, window).ok_or(ERROR_NOT_FOUND)?;
        let sources = &surface.sprites;
        surface
            .font_sprites
            .retain(|s| sources.iter().any(|(id, _)| *id == s.id));
        if let Some(sprite) = surface.font_sprites.iter().find(|s| s.key == key) {
            return Ok(u64::from(sprite.id));
        }
    }
    let height = height as u32;
    let width = (height * 6 + 5) / 11;
    let mut rgba = vec![0; (width * height * 4) as usize];
    crate::r::services::shell3_font_service::with_cell_coverage(
        character,
        width,
        height,
        color.to_le_bytes(),
        false,
        |x, y, alpha, ink| {
            let at = ((y * width + x) * 4) as usize;
            rgba[at..at + 3].copy_from_slice(&ink[..3]);
            rgba[at + 3] = ((u32::from(alpha) * u32::from(ink[3]) + 127) / 255) as u8;
        },
    )
    .map_err(|reason| {
        if reason == "shell3-font-not-ready" {
            ERROR_BUSY
        } else {
            ERROR_UI4
        }
    })?;
    let mut prepared = hull_image_api::prepare_rgba(width, height, &rgba)?;
    let id =
        hull_image_api::install_prepared(u32::from(vm), window, incarnation, 0, &mut prepared)?;
    let mut surfaces = SURFACES.lock();
    let surface = surface_mut(&mut surfaces, owner, window).ok_or(ERROR_NOT_FOUND)?;
    if surface.hull_images.incarnation != incarnation {
        return Err(ERROR_NOT_FOUND);
    }
    surface.font_sprites.push(Sprite {
        key,
        id,
        width,
        height,
    });
    Ok(u64::from(id))
}

pub(crate) fn status(
    owner: WindowOwner,
    window: u32,
    ticket: u64,
) -> Result<TrueosUi4FontSpriteStatusV1, i32> {
    let id = u32::try_from(ticket).map_err(|_| ERROR_INVALID)?;
    let mut surfaces = SURFACES.try_lock().ok_or(ERROR_BUSY)?;
    let surface = surface_mut(&mut surfaces, owner, window).ok_or(ERROR_NOT_FOUND)?;
    let sprite = surface
        .font_sprites
        .iter()
        .find(|s| s.id == id)
        .ok_or(ERROR_NOT_FOUND)?;
    if !surface.sprites.iter().any(|(source, _)| *source == id) {
        return Err(ERROR_NOT_FOUND);
    }
    Ok(TrueosUi4FontSpriteStatusV1 {
        state: 2,
        sprite_id: id,
        width: sprite.width,
        height: sprite.height,
        origin_x: 0,
        origin_y: 0,
    })
}

pub unsafe extern "C" fn trueos_cabi_ui4_scene_font_sprite_request_v1(
    window: u32,
    font: u32,
    scalar: u32,
    pixels: f32,
    color: u32,
    out: *mut u64,
) -> i32 {
    if out.is_null() {
        return ERROR_INVALID;
    }
    let mut payload = [0; 16];
    for (target, word) in payload
        .chunks_exact_mut(4)
        .zip([font, scalar, pixels.to_bits(), color])
    {
        target.copy_from_slice(&word.to_le_bytes());
    }
    let result = if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let mut response = [0; 8];
        let (transport, rc) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_UI4_SCENE_FONT_SPRITE_REQUEST_V1,
            u64::from(window),
            0,
            &payload,
            &mut response,
        );
        if transport != trueos_vm::vmcall::STATUS_OK {
            return ERROR_UI4;
        }
        if rc as i64 != 0 {
            return rc as i64 as i32;
        }
        Ok(u64::from_le_bytes(response))
    } else {
        let Some(owner) = blueprint_owner() else {
            return ERROR_CONTEXT;
        };
        request(owner, window, &payload)
    };
    match result {
        Ok(ticket) => {
            unsafe {
                out.write_unaligned(ticket);
            }
            0
        }
        Err(rc) => rc,
    }
}

pub unsafe extern "C" fn trueos_cabi_ui4_scene_font_sprite_status_v1(
    window: u32,
    ticket: u64,
    out: *mut TrueosUi4FontSpriteStatusV1,
) -> i32 {
    if out.is_null() || ticket == 0 {
        return ERROR_INVALID;
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let mut bytes = [0; 24];
        let (transport, rc) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_UI4_SCENE_FONT_SPRITE_STATUS_V1,
            u64::from(window),
            ticket,
            &[],
            &mut bytes,
        );
        if transport != trueos_vm::vmcall::STATUS_OK {
            return ERROR_UI4;
        }
        if rc as i64 != 0 {
            return rc as i64 as i32;
        }
        unsafe {
            out.write_unaligned(core::ptr::read_unaligned(bytes.as_ptr().cast()));
        }
        return 0;
    }
    let Some(owner) = blueprint_owner() else {
        return ERROR_CONTEXT;
    };
    match status(owner, window, ticket) {
        Ok(info) => {
            unsafe {
                out.write_unaligned(info);
            }
            0
        }
        Err(rc) => rc,
    }
}
