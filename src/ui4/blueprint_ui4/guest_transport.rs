//! Guest-side UI4 VM-call marshalling and synchronous retry policy.
//! Host surface ownership and GPU retirement stay in the parent module.

use alloc::vec::Vec;

use super::{
    BlueprintImageSourceInfo, CONTEXT_MENU_ENTRY_WIRE_HEADER_BYTES, CONTEXT_MENU_WIRE_HEADER_BYTES,
    ERROR_BUSY, ERROR_INVALID, ERROR_UI4, IMAGE_SOURCE_READ_CHUNK_BYTES,
    MAX_CONTEXT_MENU_LABEL_BYTES, MAX_INPUT_ROUTES, TrueosUi4ContextMenuEntry,
    TrueosUi4ContextMenuEvent, TrueosUi4InputRouteState, TrueosUi4KeyboardState, TrueosUi4PanEvent,
    TrueosUi4PointerEvent, TrueosUi4ResizeEvent,
};

pub(super) unsafe fn guest_image_source_info(
    name_ptr: *const u8,
    name_len: usize,
    out: *mut BlueprintImageSourceInfo,
) -> i32 {
    let name = unsafe { core::slice::from_raw_parts(name_ptr, name_len) };
    let mut response = [0u8; core::mem::size_of::<BlueprintImageSourceInfo>()];
    let (status, data) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_IMAGE_SOURCE_INFO,
        0,
        0,
        name,
        &mut response,
    );
    if status != trueos_vm::vmcall::STATUS_OK {
        return ERROR_UI4;
    }
    let result = data as i64 as i32;
    if result != 0 {
        return result;
    }
    unsafe { out.write(core::ptr::read_unaligned(response.as_ptr().cast())) };
    0
}

pub(super) unsafe fn guest_image_source_read(
    name_ptr: *const u8,
    name_len: usize,
    offset: usize,
    out_ptr: *mut u8,
    out_cap: usize,
) -> isize {
    if out_cap > IMAGE_SOURCE_READ_CHUNK_BYTES {
        return ERROR_INVALID as isize;
    }
    let name = unsafe { core::slice::from_raw_parts(name_ptr, name_len) };
    let mut response = alloc::vec![0u8; out_cap];
    let (status, data) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_IMAGE_SOURCE_READ,
        offset as u64,
        out_cap as u64,
        name,
        response.as_mut_slice(),
    );
    if status != trueos_vm::vmcall::STATUS_OK {
        return ERROR_UI4 as isize;
    }
    let copied = data as usize;
    if copied > out_cap {
        return ERROR_UI4 as isize;
    }
    unsafe { core::ptr::copy_nonoverlapping(response.as_ptr(), out_ptr, copied) };
    copied as isize
}

pub(super) unsafe fn guest_context_menu_register(
    window_id: u32,
    entries: &[TrueosUi4ContextMenuEntry],
    dynamic: bool,
    serial: u64,
) -> i32 {
    let mut payload = Vec::with_capacity(
        CONTEXT_MENU_WIRE_HEADER_BYTES.saturating_add(
            entries
                .len()
                .saturating_mul(CONTEXT_MENU_ENTRY_WIRE_HEADER_BYTES + 16),
        ),
    );
    payload.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    for entry in entries {
        if entry.label_ptr.is_null()
            || entry.label_len == 0
            || entry.label_len > MAX_CONTEXT_MENU_LABEL_BYTES
        {
            return ERROR_INVALID;
        }
        let Some(required) = payload
            .len()
            .checked_add(CONTEXT_MENU_ENTRY_WIRE_HEADER_BYTES)
            .and_then(|bytes| bytes.checked_add(entry.label_len))
        else {
            return ERROR_INVALID;
        };
        if required > trueos_vm::vmcall::PAYLOAD_CAP {
            return ERROR_INVALID;
        }
        // SAFETY: the C ABI requires `label_len` readable bytes per entry.
        let label = unsafe { core::slice::from_raw_parts(entry.label_ptr, entry.label_len) };
        payload.extend_from_slice(&entry.action_id.to_le_bytes());
        payload.extend_from_slice(&entry.enabled.to_le_bytes());
        payload.extend_from_slice(&(entry.label_len as u32).to_le_bytes());
        payload.extend_from_slice(label);
    }
    guest_status(
        trueos_vm::vmcall::OP_BP_UI4_CONTEXT_MENU_REGISTER,
        window_id as u64 | (u64::from(dynamic) << 32),
        serial,
        payload.as_slice(),
    )
}

pub(super) unsafe fn guest_context_menu_event_take(
    window_id: u32,
    out: *mut TrueosUi4ContextMenuEvent,
) -> i32 {
    let mut response = [0u8; core::mem::size_of::<TrueosUi4ContextMenuEvent>()];
    let (status, data) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_UI4_CONTEXT_MENU_EVENT_TAKE,
        window_id as u64,
        0,
        &[],
        &mut response,
    );
    if status != trueos_vm::vmcall::STATUS_OK {
        return ERROR_UI4;
    }
    let result = data as i64 as i32;
    if result != 0 {
        return result;
    }
    let event = unsafe { core::ptr::read_unaligned(response.as_ptr().cast()) };
    unsafe { out.write(event) };
    0
}

pub(super) unsafe fn guest_context_menu_event_take_v2(
    window_id: u32,
    out: *mut super::TrueosUi4ContextMenuEventV2,
) -> i32 {
    let mut response = [0u8; core::mem::size_of::<super::TrueosUi4ContextMenuEventV2>()];
    let (status, data) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_UI4_CONTEXT_MENU_EVENT_TAKE,
        window_id as u64,
        1,
        &[],
        &mut response,
    );
    if status != trueos_vm::vmcall::STATUS_OK {
        return ERROR_UI4;
    }
    let result = data as i64 as i32;
    if result != 0 {
        return result;
    }
    unsafe { out.write(core::ptr::read_unaligned(response.as_ptr().cast())) };
    0
}

pub(super) unsafe fn guest_pointer_event_take(
    window_id: u32,
    out: *mut TrueosUi4PointerEvent,
) -> i32 {
    let mut response = [0u8; core::mem::size_of::<TrueosUi4PointerEvent>()];
    let (status, data) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_UI4_SCENE_POINTER_EVENT_TAKE,
        window_id as u64,
        0,
        &[],
        &mut response,
    );
    if status != trueos_vm::vmcall::STATUS_OK {
        return ERROR_UI4;
    }
    let result = data as i64 as i32;
    if result != 0 {
        return result;
    }
    let event = unsafe { core::ptr::read_unaligned(response.as_ptr().cast()) };
    unsafe { out.write(event) };
    0
}

pub(super) unsafe fn guest_keyboard_event_take(
    window_id: u32,
    out: *mut crate::r::keyboard::TrueosKeyboardOutputEvent,
) -> i32 {
    let mut response = [0u8; core::mem::size_of::<crate::r::keyboard::TrueosKeyboardOutputEvent>()];
    let (status, data) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_UI4_SCENE_KEYBOARD_EVENT_TAKE,
        window_id as u64,
        0,
        &[],
        &mut response,
    );
    if status != trueos_vm::vmcall::STATUS_OK {
        return ERROR_UI4;
    }
    let result = data as i64 as i32;
    if result != 0 {
        return result;
    }
    let event = unsafe { core::ptr::read_unaligned(response.as_ptr().cast()) };
    unsafe { out.write(event) };
    0
}

pub(super) unsafe fn guest_keyboard_event_take_v1(
    window_id: u32,
    out: *mut crate::r::keyboard::TrueosKeyboardOutputEvent,
) -> i32 {
    let mut response = [0u8; core::mem::size_of::<crate::r::keyboard::TrueosKeyboardOutputEvent>()];
    let (status, data) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_UI4_SCENE_KEYBOARD_EVENT_TAKE_V1,
        window_id as u64,
        0,
        &[],
        &mut response,
    );
    if status != trueos_vm::vmcall::STATUS_OK {
        return ERROR_UI4;
    }
    let result = data as i64 as i32;
    if result != 0 {
        return result;
    }
    let event = unsafe { core::ptr::read_unaligned(response.as_ptr().cast()) };
    unsafe { out.write(event) };
    0
}

pub(super) unsafe fn guest_pan_event_take(window_id: u32, out: *mut TrueosUi4PanEvent) -> i32 {
    let mut response = [0u8; core::mem::size_of::<TrueosUi4PanEvent>()];
    let (status, data) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_UI4_SCENE_PAN_EVENT_TAKE,
        window_id as u64,
        0,
        &[],
        &mut response,
    );
    if status != trueos_vm::vmcall::STATUS_OK {
        return ERROR_UI4;
    }
    let result = data as i64 as i32;
    if result != 0 {
        return result;
    }
    let event = unsafe { core::ptr::read_unaligned(response.as_ptr().cast()) };
    unsafe { out.write(event) };
    0
}

pub(super) unsafe fn guest_resize_event_take(
    window_id: u32,
    out: *mut TrueosUi4ResizeEvent,
) -> i32 {
    let mut response = [0u8; core::mem::size_of::<TrueosUi4ResizeEvent>()];
    let (status, data) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_UI4_SCENE_RESIZE_EVENT_TAKE,
        window_id as u64,
        0,
        &[],
        &mut response,
    );
    if status != trueos_vm::vmcall::STATUS_OK {
        return ERROR_UI4;
    }
    let result = data as i64 as i32;
    if result != 0 {
        return result;
    }
    let event = unsafe { core::ptr::read_unaligned(response.as_ptr().cast()) };
    unsafe { out.write(event) };
    0
}

pub(super) unsafe fn guest_keyboard_state(window_id: u32, out: *mut TrueosUi4KeyboardState) -> i32 {
    let mut response = [0u8; core::mem::size_of::<TrueosUi4KeyboardState>()];
    let (status, data) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_UI4_SCENE_KEYBOARD_STATE,
        window_id as u64,
        0,
        &[],
        &mut response,
    );
    if status != trueos_vm::vmcall::STATUS_OK {
        return ERROR_UI4;
    }
    let result = data as i64 as i32;
    if result != 0 {
        return result;
    }
    let state = unsafe { core::ptr::read_unaligned(response.as_ptr().cast()) };
    unsafe { out.write(state) };
    0
}

pub(super) unsafe fn guest_input_routes(
    window_id: u32,
    out: *mut TrueosUi4InputRouteState,
    out_cap: u32,
) -> isize {
    let response_cap = (out_cap as usize).min(MAX_INPUT_ROUTES);
    let mut response =
        alloc::vec![0u8; response_cap * core::mem::size_of::<TrueosUi4InputRouteState>()];
    let (status, data) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_UI4_SCENE_INPUT_ROUTES,
        window_id as u64,
        response_cap as u64,
        &[],
        response.as_mut_slice(),
    );
    if status != trueos_vm::vmcall::STATUS_OK {
        return ERROR_UI4 as isize;
    }
    let result = data as i64;
    if result < 0 {
        return result as isize;
    }
    let copied = (result as usize).min(response_cap);
    if copied != 0 {
        // SAFETY: out_cap covers response_cap records and the vmcall copied
        // exactly the initialized response bytes for `copied` records.
        unsafe {
            core::ptr::copy_nonoverlapping(
                response.as_ptr(),
                out.cast::<u8>(),
                copied * core::mem::size_of::<TrueosUi4InputRouteState>(),
            );
        }
    }
    result as isize
}

pub(super) fn guest_status(op: u32, arg0: u64, arg1: u64, payload: &[u8]) -> i32 {
    let (status, data) = trueos_vm::vmcall::call_with_payload(op, arg0, arg1, payload, &mut []);
    if status == trueos_vm::vmcall::STATUS_OK {
        data as i64 as i32
    } else {
        ERROR_UI4
    }
}
