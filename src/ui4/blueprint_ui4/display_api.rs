//! Native display handles name a live connection to the caller's UI4 service.

use spin::Mutex;

use super::{
    ERROR_CONTEXT, ERROR_NOT_FOUND, ERROR_UI4, SURFACES, blueprint_owner,
    display_registry::DisplayRegistry, surface_mut,
};
use crate::ui4::WindowOwner;

static DISPLAYS: Mutex<DisplayRegistry<WindowOwner, 64>> = Mutex::new(DisplayRegistry::new());

pub(super) fn release_owner(owner: WindowOwner) {
    DISPLAYS.lock().release_owner(owner);
}

fn guest_call(op: u32, connection: u64, window: u32) -> (u32, u64) {
    trueos_vm::vmcall::call_with_payload(op, connection, window as u64, &[], &mut [])
}

pub extern "C" fn trueos_cabi_ui4_display_open_v1() -> u64 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, token) = guest_call(trueos_vm::vmcall::OP_BP_UI4_DISPLAY_OPEN_V1, 0, 0);
        return if status == trueos_vm::vmcall::STATUS_OK {
            token
        } else {
            0
        };
    }
    let Some(owner) = blueprint_owner() else {
        return 0;
    };
    DISPLAYS.lock().open(owner).unwrap_or(0)
}

pub extern "C" fn trueos_cabi_ui4_display_retain_v1(connection: u64) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, result) =
            guest_call(trueos_vm::vmcall::OP_BP_UI4_DISPLAY_RETAIN_V1, connection, 0);
        return if status == trueos_vm::vmcall::STATUS_OK {
            result as i64 as i32
        } else {
            ERROR_UI4
        };
    }
    let Some(owner) = blueprint_owner() else {
        return ERROR_CONTEXT;
    };
    if DISPLAYS.lock().retain(owner, connection) {
        0
    } else {
        ERROR_NOT_FOUND
    }
}

pub extern "C" fn trueos_cabi_ui4_display_close_v1(connection: u64) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, result) =
            guest_call(trueos_vm::vmcall::OP_BP_UI4_DISPLAY_CLOSE_V1, connection, 0);
        return if status == trueos_vm::vmcall::STATUS_OK {
            result as i64 as i32
        } else {
            ERROR_UI4
        };
    }
    let Some(owner) = blueprint_owner() else {
        return ERROR_CONTEXT;
    };
    if DISPLAYS.lock().close(owner, connection) {
        0
    } else {
        ERROR_NOT_FOUND
    }
}

pub extern "C" fn trueos_cabi_ui4_display_validate_window_v1(connection: u64, window: u32) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, result) =
            guest_call(trueos_vm::vmcall::OP_BP_UI4_DISPLAY_VALIDATE_WINDOW_V1, connection, window);
        return if status == trueos_vm::vmcall::STATUS_OK {
            result as i64 as i32
        } else {
            ERROR_UI4
        };
    }
    let Some(owner) = blueprint_owner() else {
        return ERROR_CONTEXT;
    };
    if !DISPLAYS.lock().valid(owner, connection) {
        return ERROR_NOT_FOUND;
    }
    let surfaces = &mut *SURFACES.lock();
    let Some(surface) = surface_mut(surfaces, owner, window) else {
        return ERROR_NOT_FOUND;
    };
    if crate::ui4::window_state(owner, surface.window).is_ok() {
        0
    } else {
        ERROR_NOT_FOUND
    }
}
