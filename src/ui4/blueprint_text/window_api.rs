//! Narrow, broker-enforced window metadata for Blueprint scene frames.
//!
//! The parent ABI entry points perform guest transport first, then call these
//! host helpers.  No broker handle crosses the ABI boundary.

use super::{ERROR_CONTEXT, ERROR_INVALID, ERROR_NOT_FOUND, ERROR_UI4, SURFACES, blueprint_owner};
use crate::ui4::{
    MAX_WINDOW_TITLE_BYTES, focused_keyboard_state, set_window_state, set_window_title,
    window_state, window_title,
};

pub(crate) const WINDOW_STATE_V1_VERSION: u32 = 1;

/// Exact window state exposed to an application. `focused` reflects the
/// currently routed keyboard state; it does not manufacture focus from a
/// pointer event.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct TrueosUi4WindowStateV1 {
    pub version: u32,
    pub visible: u32,
    pub hit_testable: u32,
    pub opacity: u32,
    pub focused: u32,
    pub reserved: [u32; 3],
}

fn owned_window(window_id: u32) -> Result<(crate::ui4::WindowOwner, crate::ui4::WindowId), i32> {
    let owner = blueprint_owner().ok_or(ERROR_CONTEXT)?;
    let surfaces = SURFACES.lock();
    let surface = surfaces
        .iter()
        .find(|surface| surface.owner == owner && surface.window.raw() == window_id)
        .ok_or(ERROR_NOT_FOUND)?;
    Ok((owner, surface.window))
}

pub(super) unsafe fn get_state(window_id: u32, out: *mut TrueosUi4WindowStateV1) -> i32 {
    if out.is_null() {
        return ERROR_INVALID;
    }
    let (owner, window) = match owned_window(window_id) {
        Ok(window) => window,
        Err(error) => return error,
    };
    let Ok((placement, interaction)) = window_state(owner, window) else {
        return ERROR_UI4;
    };
    let focused = focused_keyboard_state(owner, window).is_some();
    unsafe {
        out.write(TrueosUi4WindowStateV1 {
            version: WINDOW_STATE_V1_VERSION,
            visible: placement.visible as u32,
            hit_testable: interaction.hit_testable as u32,
            opacity: placement.opacity as u32,
            focused: focused as u32,
            reserved: [0; 3],
        });
    }
    0
}

pub(super) fn set_state(window_id: u32, state: &TrueosUi4WindowStateV1) -> i32 {
    if state.version != WINDOW_STATE_V1_VERSION
        || state.visible > 1
        || state.hit_testable > 1
        || state.opacity > u8::MAX as u32
        || state.reserved != [0; 3]
    {
        return ERROR_INVALID;
    }
    let (owner, window) = match owned_window(window_id) {
        Ok(window) => window,
        Err(error) => return error,
    };
    set_window_state(
        owner,
        window,
        state.visible != 0,
        state.hit_testable != 0,
        state.opacity as u8,
    )
    .map(|()| 0)
    .unwrap_or(ERROR_UI4)
}

/// Copies a UTF-8 title into `out`; returns its length or a negative ABI error.
pub(super) unsafe fn get_title(window_id: u32, out: *mut u8, out_cap: usize) -> isize {
    if out.is_null() || out_cap < MAX_WINDOW_TITLE_BYTES {
        return ERROR_INVALID as isize;
    }
    let (owner, window) = match owned_window(window_id) {
        Ok(window) => window,
        Err(error) => return error as isize,
    };
    let mut title = [0u8; MAX_WINDOW_TITLE_BYTES];
    let Ok(len) = window_title(owner, window, &mut title) else {
        return ERROR_UI4 as isize;
    };
    unsafe { core::ptr::copy_nonoverlapping(title.as_ptr(), out, len) };
    len as isize
}

pub(super) unsafe fn set_title(window_id: u32, bytes: *const u8, len: usize) -> i32 {
    if bytes.is_null() || len > MAX_WINDOW_TITLE_BYTES {
        return ERROR_INVALID;
    }
    let Ok(title) = core::str::from_utf8(unsafe { core::slice::from_raw_parts(bytes, len) }) else {
        return ERROR_INVALID;
    };
    let (owner, window) = match owned_window(window_id) {
        Ok(window) => window,
        Err(error) => return error,
    };
    set_window_title(owner, window, title)
        .map(|()| 0)
        .unwrap_or(ERROR_UI4)
}
