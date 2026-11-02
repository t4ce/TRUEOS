//! Small owner-checked hit-region signal to the kernel interaction plane.
use super::{
    ERROR_CONTEXT, ERROR_INVALID, ERROR_NOT_FOUND, SURFACES, blueprint_owner, guest_status,
    surface_mut,
};
use v::bp_abi::TrueosUi4ButtonRegionV1 as Region;

pub unsafe extern "C" fn trueos_cabi_ui4_scene_button_regions_v1(
    window_id: u32,
    regions: *const Region,
    count: usize,
) -> i32 {
    if count > 256 || (count != 0 && regions.is_null()) {
        return ERROR_INVALID;
    }
    let mut copied = alloc::vec::Vec::with_capacity(count);
    for index in 0..count {
        copied.push(unsafe { regions.add(index).read_unaligned() });
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let bytes =
            unsafe { core::slice::from_raw_parts(copied.as_ptr().cast::<u8>(), copied.len() * 36) };
        return guest_status(
            trueos_vm::vmcall::OP_BP_UI4_SCENE_BUTTON_REGIONS_V1,
            window_id as u64,
            count as u64,
            bytes,
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
    crate::ui4::button_outline::register(snapshot, &copied).map_or_else(|e| e, |_| 0)
}
