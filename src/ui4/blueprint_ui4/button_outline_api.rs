//! Small owner-checked hit-region signal to the kernel interaction plane.
use super::{
    ERROR_CONTEXT, ERROR_INVALID, ERROR_NOT_FOUND, SURFACES, blueprint_owner, guest_status,
    surface_mut,
};
use v::bp_abi::TrueosUi4ButtonRegionV1 as Region;

const REGION_WIRE_BYTES: usize = 36;
const _: () = assert!(core::mem::size_of::<Region>() == REGION_WIRE_BYTES);
const _: () = assert!(core::mem::offset_of!(Region, x) == 0);
const _: () = assert!(core::mem::offset_of!(Region, y) == 4);
const _: () = assert!(core::mem::offset_of!(Region, width) == 8);
const _: () = assert!(core::mem::offset_of!(Region, height) == 12);
const _: () = assert!(core::mem::offset_of!(Region, clip_x) == 16);
const _: () = assert!(core::mem::offset_of!(Region, clip_y) == 20);
const _: () = assert!(core::mem::offset_of!(Region, clip_width) == 24);
const _: () = assert!(core::mem::offset_of!(Region, clip_height) == 28);
const _: () = assert!(core::mem::offset_of!(Region, border_rgba) == 32);

fn encode_regions(regions: &[Region]) -> alloc::vec::Vec<u8> {
    let mut bytes = alloc::vec::Vec::with_capacity(regions.len() * REGION_WIRE_BYTES);
    for region in regions {
        bytes.extend_from_slice(&region.x.to_le_bytes());
        bytes.extend_from_slice(&region.y.to_le_bytes());
        bytes.extend_from_slice(&region.width.to_le_bytes());
        bytes.extend_from_slice(&region.height.to_le_bytes());
        bytes.extend_from_slice(&region.clip_x.to_le_bytes());
        bytes.extend_from_slice(&region.clip_y.to_le_bytes());
        bytes.extend_from_slice(&region.clip_width.to_le_bytes());
        bytes.extend_from_slice(&region.clip_height.to_le_bytes());
        bytes.extend_from_slice(&region.border_rgba.to_le_bytes());
    }
    bytes
}

pub unsafe extern "C" fn trueos_cabi_ui4_scene_button_regions_v1(
    window_id: u32,
    regions: *const Region,
    count: usize,
) -> i32 {
    unsafe { set_regions(window_id, regions, count, false) }
}

/// Hover-only outlines use the supplied ink color and never become pressed.
pub unsafe extern "C" fn trueos_cabi_ui4_scene_hover_regions_v1(
    window_id: u32,
    regions: *const Region,
    count: usize,
) -> i32 {
    unsafe { set_regions(window_id, regions, count, true) }
}

unsafe fn set_regions(
    window_id: u32,
    regions: *const Region,
    count: usize,
    hover_only: bool,
) -> i32 {
    if count > 256 || (count != 0 && regions.is_null()) {
        return ERROR_INVALID;
    }
    let mut copied = alloc::vec::Vec::with_capacity(count);
    for index in 0..count {
        copied.push(unsafe { regions.add(index).read_unaligned() });
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let bytes = encode_regions(&copied);
        return guest_status(
            if hover_only {
                trueos_vm::vmcall::OP_BP_UI4_SCENE_HOVER_REGIONS_V1
            } else {
                trueos_vm::vmcall::OP_BP_UI4_SCENE_BUTTON_REGIONS_V1
            },
            window_id as u64,
            count as u64,
            &bytes,
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
    let Some(snapshot) = crate::ui4::window_broker::window_snapshot(owner, window) else {
        return ERROR_NOT_FOUND;
    };
    let result = if hover_only {
        crate::ui4::button_outline::register_hover(snapshot, &copied)
    } else {
        crate::ui4::button_outline::register(snapshot, &copied)
    };
    result.map_or_else(|e| e, |_| 0)
}
