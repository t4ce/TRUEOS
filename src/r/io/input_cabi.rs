extern crate alloc;

use crate::r::gamepad_control_service::{
    GamepadControlPrincipal, gamepad_is_idle, gamepad_snapshot, release_gamepad, request_gamepad,
    submit_command as submit_gamepad_command, submit_json as submit_gamepad_json,
};
use crate::r::keyboard_control_service::{
    KeyboardControlPrincipal, keyboard_is_idle, release_keyboard, request_keyboard,
    submit_command as submit_keyboard_command, submit_json as submit_keyboard_json,
    submit_text as submit_keyboard_text,
};
use crate::r::mouse_motion_service::{
    MouseControlPrincipal, legacy_write_cursor, release_cursor, request_cursor, submit_command,
    submit_json,
};

fn input_combo_source_kind(value: u8) -> crate::usb2::hid::hut::HidSourceKind {
    match value {
        1 => crate::usb2::hid::hut::HidSourceKind::Human,
        2 => crate::usb2::hid::hut::HidSourceKind::Ai,
        3 => crate::usb2::hid::hut::HidSourceKind::Remote,
        _ => crate::usb2::hid::hut::HidSourceKind::Unknown,
    }
}

fn input_combo_info(
    combo: &crate::usb2::hid::hut::HidCombo,
) -> v::vinput::TrueosHidHutCombo {
    let mut out = v::vinput::TrueosHidHutCombo {
        combo_id: combo.combo_id,
        source_kind: combo.source_kind as u8,
        color_id: combo.color_id,
        flags: combo.flags,
        mouse_controller_id: combo.mouse_controller_id,
        mouse_slot_id: combo.mouse_slot_id,
        mouse_ep_target: combo.mouse_ep_target,
        keyboard_controller_id: combo.keyboard_controller_id,
        keyboard_slot_id: combo.keyboard_slot_id,
        keyboard_ep_target: combo.keyboard_ep_target,
        tablet_controller_id: combo.tablet_controller_id,
        tablet_slot_id: combo.tablet_slot_id,
        tablet_ep_target: combo.tablet_ep_target,
        gamepad_controller_id: combo.gamepad_controller_id,
        gamepad_slot_id: combo.gamepad_slot_id,
        gamepad_ep_target: combo.gamepad_ep_target,
        ..v::vinput::TrueosHidHutCombo::default()
    };
    let source_tag = combo.source_tag.as_bytes();
    let source_tag_len = core::cmp::min(source_tag.len(), out.source_tag.len());
    out.source_tag[..source_tag_len].copy_from_slice(&source_tag[..source_tag_len]);
    out.source_tag_len = source_tag_len as u8;
    out
}

unsafe fn input_cursor_buttons(cursor_id: u32, out_buttons_down: *mut u32) -> i32 {
    if out_buttons_down.is_null() || cursor_id == 0 {
        return -1;
    }

    let Some(buttons_down) = crate::r::cursor::cursor_buttons(cursor_id) else {
        return 1;
    };
    unsafe {
        *out_buttons_down = buttons_down;
    }
    0
}

pub fn host_input_cursor_buttons(cursor_id: u32, out_buttons_down: &mut u32) -> i32 {
    if cursor_id == 0 {
        return -1;
    }
    let Some(buttons_down) = crate::r::cursor::cursor_buttons(cursor_id) else {
        return 1;
    };
    *out_buttons_down = buttons_down;
    0
}

pub fn host_input_cursor_pos(cursor_id: u32, out_x: &mut i32, out_y: &mut i32) -> i32 {
    if cursor_id == 0 {
        return -1;
    }

    let Some((nx, ny)) = crate::r::cursor::cursor_pos(cursor_id) else {
        return 1;
    };

    let (w, h) = cursor_viewport_dimensions();
    let w1 = w.saturating_sub(1) as f64;
    let h1 = h.saturating_sub(1) as f64;

    *out_x = libm::round(nx * w1) as i32;
    *out_y = libm::round(ny * h1) as i32;
    0
}

pub fn host_input_cursor_events_since(
    read_seq: u64,
    out_cap: u32,
    payload: &mut [u8],
) -> (usize, usize) {
    const HEADER_LEN: usize = 12;
    let event_size = core::mem::size_of::<crate::usb2::hid::TrueosHidCursorEvent>();
    if payload.len() < HEADER_LEN || event_size == 0 {
        return (0, 0);
    }
    let max_events = (payload.len() - HEADER_LEN) / event_size;
    let cap = core::cmp::min(out_cap as usize, max_events);
    let mut events = alloc::vec![
        crate::usb2::hid::TrueosHidCursorEvent::default();
        cap
    ];
    let (next_seq, dropped, wrote) =
        crate::usb2::hid::read_cursor_events_since(read_seq, events.as_mut_slice());
    payload[0..8].copy_from_slice(&next_seq.to_le_bytes());
    payload[8..12].copy_from_slice(&dropped.to_le_bytes());
    let bytes_len = wrote.saturating_mul(event_size);
    if bytes_len != 0 {
        let bytes = unsafe { core::slice::from_raw_parts(events.as_ptr() as *const u8, bytes_len) };
        payload[HEADER_LEN..HEADER_LEN + bytes_len].copy_from_slice(bytes);
    }
    (wrote, HEADER_LEN + wrote.saturating_mul(event_size))
}

pub fn host_input_pop_keyboard_output(payload: &mut [u8]) -> (i32, usize) {
    let event_size = core::mem::size_of::<crate::r::keyboard::TrueosKeyboardOutputEvent>();
    if payload.len() < event_size {
        return (-1, 0);
    }
    let Some(event) = crate::r::keyboard::pop_output_event() else {
        return (1, 0);
    };
    let bytes = unsafe { core::slice::from_raw_parts(&event as *const _ as *const u8, event_size) };
    payload[..event_size].copy_from_slice(bytes);
    (0, event_size)
}

pub fn host_input_keyboard_output_since(
    read_seq: u64,
    out_cap: u32,
    payload: &mut [u8],
) -> (usize, usize) {
    const HEADER_LEN: usize = 12;
    let event_size = core::mem::size_of::<crate::r::keyboard::TrueosKeyboardOutputEvent>();
    if payload.len() < HEADER_LEN || event_size == 0 {
        return (0, 0);
    }
    let max_events = (payload.len() - HEADER_LEN) / event_size;
    let cap = core::cmp::min(out_cap as usize, max_events);
    let mut events = alloc::vec![crate::r::keyboard::TrueosKeyboardOutputEvent::default(); cap];
    let (next_seq, dropped, wrote) =
        crate::r::keyboard::read_output_events_since(read_seq, events.as_mut_slice());
    payload[0..8].copy_from_slice(&next_seq.to_le_bytes());
    payload[8..12].copy_from_slice(&dropped.to_le_bytes());
    let bytes_len = wrote.saturating_mul(event_size);
    if bytes_len != 0 {
        let bytes = unsafe { core::slice::from_raw_parts(events.as_ptr() as *const u8, bytes_len) };
        payload[HEADER_LEN..HEADER_LEN + bytes_len].copy_from_slice(bytes);
    }
    (wrote, HEADER_LEN + bytes_len)
}

fn guest_input_cursor_buttons(cursor_id: u32, out_buttons_down: *mut u32) -> i32 {
    if out_buttons_down.is_null() || cursor_id == 0 {
        return -1;
    }
    let (status, data) =
        trueos_vm::vmcall::call(trueos_vm::vmcall::OP_BP_INPUT_CURSOR_BUTTONS, cursor_id as u64, 0);
    if status != trueos_vm::vmcall::STATUS_OK {
        return -1;
    }
    let rc = (data >> 32) as u32 as i32;
    if rc == 0 {
        unsafe {
            *out_buttons_down = data as u32;
        }
    }
    rc
}

fn guest_input_cursor_pos(cursor_id: u32, out_x: *mut i32, out_y: *mut i32) -> i32 {
    if out_x.is_null() || out_y.is_null() || cursor_id == 0 {
        return -1;
    }
    let (status, data) =
        trueos_vm::vmcall::call(trueos_vm::vmcall::OP_BP_INPUT_CURSOR_POS, cursor_id as u64, 0);
    if status != trueos_vm::vmcall::STATUS_OK {
        return data as i64 as i32;
    }
    unsafe {
        *out_x = (data >> 32) as u32 as i32;
        *out_y = data as u32 as i32;
    }
    0
}

fn guest_input_read_cursor_events_since(
    read_seq: u64,
    out: *mut crate::usb2::hid::TrueosHidCursorEvent,
    out_cap: u32,
    out_next_seq: *mut u64,
    out_dropped: *mut u32,
) -> u32 {
    if out_next_seq.is_null() || out_dropped.is_null() {
        return 0;
    }
    let mut payload = [0u8; trueos_vm::vmcall::PAYLOAD_CAP];
    let (status, wrote) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_INPUT_CURSOR_EVENTS,
        read_seq,
        out_cap as u64,
        &[],
        &mut payload,
    );
    if status != trueos_vm::vmcall::STATUS_OK || payload.len() < 12 {
        return 0;
    }
    let next_seq = u64::from_le_bytes([
        payload[0], payload[1], payload[2], payload[3], payload[4], payload[5], payload[6],
        payload[7],
    ]);
    let dropped = u32::from_le_bytes([payload[8], payload[9], payload[10], payload[11]]);
    unsafe {
        *out_next_seq = next_seq;
        *out_dropped = dropped;
    }

    let event_size = core::mem::size_of::<crate::usb2::hid::TrueosHidCursorEvent>();
    let got = core::cmp::min(wrote as usize, out_cap as usize);
    let bytes_len = got.saturating_mul(event_size);
    if got == 0 || out.is_null() || payload.len() < 12 + bytes_len {
        return got as u32;
    }
    unsafe {
        core::ptr::copy_nonoverlapping(
            payload[12..12 + bytes_len].as_ptr(),
            out as *mut u8,
            bytes_len,
        );
    }
    got as u32
}

fn guest_input_pop_keyboard_output(out: *mut crate::r::keyboard::TrueosKeyboardOutputEvent) -> i32 {
    if out.is_null() {
        return -1;
    }
    let mut payload = [0u8; trueos_vm::vmcall::PAYLOAD_CAP];
    let (status, data) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_INPUT_KEYBOARD_OUTPUT_POP,
        0,
        0,
        &[],
        &mut payload,
    );
    if status != trueos_vm::vmcall::STATUS_OK {
        return -1;
    }
    let rc = data as i64 as i32;
    if rc != 0 {
        return rc;
    }
    let event_size = core::mem::size_of::<crate::r::keyboard::TrueosKeyboardOutputEvent>();
    unsafe {
        core::ptr::copy_nonoverlapping(payload.as_ptr(), out as *mut u8, event_size);
    }
    0
}

fn guest_input_read_keyboard_output_since(
    read_seq: u64,
    out: *mut crate::r::keyboard::TrueosKeyboardOutputEvent,
    out_cap: u32,
    out_next_seq: *mut u64,
    out_dropped: *mut u32,
) -> u32 {
    if out_next_seq.is_null() || out_dropped.is_null() {
        return 0;
    }
    let mut payload = [0u8; trueos_vm::vmcall::PAYLOAD_CAP];
    let (status, wrote) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_INPUT_KEYBOARD_OUTPUT_SINCE,
        read_seq,
        out_cap as u64,
        &[],
        &mut payload,
    );
    if status != trueos_vm::vmcall::STATUS_OK || payload.len() < 12 {
        return 0;
    }
    let next_seq = u64::from_le_bytes([
        payload[0], payload[1], payload[2], payload[3], payload[4], payload[5], payload[6],
        payload[7],
    ]);
    let dropped = u32::from_le_bytes([payload[8], payload[9], payload[10], payload[11]]);
    unsafe {
        *out_next_seq = next_seq;
        *out_dropped = dropped;
    }

    let event_size = core::mem::size_of::<crate::r::keyboard::TrueosKeyboardOutputEvent>();
    let got = core::cmp::min(wrote as usize, out_cap as usize);
    let bytes_len = got.saturating_mul(event_size);
    if got == 0 || out.is_null() || payload.len() < 12 + bytes_len {
        return got as u32;
    }
    unsafe {
        core::ptr::copy_nonoverlapping(
            payload[12..12 + bytes_len].as_ptr(),
            out as *mut u8,
            bytes_len,
        );
    }
    got as u32
}

unsafe fn input_pop_cursor_event(out: *mut crate::usb2::hid::TrueosHidCursorEvent) -> i32 {
    if out.is_null() {
        return -1;
    }
    let Some(ev) = crate::usb2::hid::pop_cursor_event() else {
        return 0;
    };
    unsafe {
        *out = ev;
    }
    1
}

unsafe fn input_read_cursor_events_since(
    read_seq: u64,
    out: *mut crate::usb2::hid::TrueosHidCursorEvent,
    out_cap: u32,
    out_next_seq: *mut u64,
    out_dropped: *mut u32,
) -> u32 {
    if out_next_seq.is_null() || out_dropped.is_null() {
        return 0;
    }

    let cap = out_cap as usize;
    if cap == 0 || out.is_null() {
        let mut none: [crate::usb2::hid::TrueosHidCursorEvent; 0] = [];
        let (next_seq, dropped, _wrote) =
            crate::usb2::hid::read_cursor_events_since(read_seq, &mut none);
        unsafe {
            *out_next_seq = next_seq;
            *out_dropped = dropped;
        }
        return 0;
    }

    let out_slice = unsafe { core::slice::from_raw_parts_mut(out, cap) };
    let (next_seq, dropped, wrote) =
        crate::usb2::hid::read_cursor_events_since(read_seq, out_slice);
    unsafe {
        *out_next_seq = next_seq;
        *out_dropped = dropped;
    }
    wrote as u32
}

unsafe fn input_pop_keyboard_output(
    out: *mut crate::r::keyboard::TrueosKeyboardOutputEvent,
) -> i32 {
    if out.is_null() {
        return -1;
    }
    let Some(event) = crate::r::keyboard::pop_output_event() else {
        return 1;
    };
    unsafe {
        *out = event;
    }
    0
}

unsafe fn input_read_keyboard_output_since(
    read_seq: u64,
    out: *mut crate::r::keyboard::TrueosKeyboardOutputEvent,
    out_cap: u32,
    out_next_seq: *mut u64,
    out_dropped: *mut u32,
) -> u32 {
    if out_next_seq.is_null() || out_dropped.is_null() {
        return 0;
    }

    let cap = out_cap as usize;
    if cap == 0 || out.is_null() {
        let mut none: [crate::r::keyboard::TrueosKeyboardOutputEvent; 0] = [];
        let (next_seq, dropped, _wrote) =
            crate::r::keyboard::read_output_events_since(read_seq, &mut none);
        unsafe {
            *out_next_seq = next_seq;
            *out_dropped = dropped;
        }
        return 0;
    }

    let out_slice = unsafe { core::slice::from_raw_parts_mut(out, cap) };
    let (next_seq, dropped, wrote) =
        crate::r::keyboard::read_output_events_since(read_seq, out_slice);
    unsafe {
        *out_next_seq = next_seq;
        *out_dropped = dropped;
    }
    wrote as u32
}

#[inline]
fn cursor_viewport_dimensions() -> (usize, usize) {
    crate::intel::active_scanout_dimensions()
        .map(|(w, h)| (w as usize, h as usize))
        .or_else(|| {
            crate::limine::framebuffer_response()
                .and_then(|resp| resp.framebuffers().first().copied())
                .map(|fb| (fb.width as usize, fb.height as usize))
        })
        .unwrap_or((320, 200))
}

pub fn input_cursor_viewport_dimensions_px() -> (i32, i32) {
    let (w, h) = cursor_viewport_dimensions();
    let w = w.min(i32::MAX as usize) as i32;
    let h = h.min(i32::MAX as usize) as i32;
    (w, h)
}

fn input_write_cursor_event(
    slot_id: u32,
    x_px: i32,
    y_px: i32,
    buttons_down: u32,
    wheel: i32,
    flags: u32,
) -> i32 {
    if slot_id == 0 {
        return -1;
    }
    legacy_write_cursor(mouse_motion_principal(), slot_id, x_px, y_px, buttons_down, wheel, flags)
        .map(|()| 0)
        .unwrap_or_else(|error| error.code())
}

fn mouse_motion_principal() -> MouseControlPrincipal {
    crate::hv::current_guest_execution_context_vm_id()
        .map(MouseControlPrincipal::Vm)
        .unwrap_or(MouseControlPrincipal::Kernel)
}

fn keyboard_control_principal() -> KeyboardControlPrincipal {
    crate::hv::current_guest_execution_context_vm_id()
        .map(KeyboardControlPrincipal::Vm)
        .unwrap_or(KeyboardControlPrincipal::Kernel)
}

fn gamepad_control_principal() -> GamepadControlPrincipal {
    crate::hv::current_guest_execution_context_vm_id()
        .map(GamepadControlPrincipal::Vm)
        .unwrap_or(GamepadControlPrincipal::Kernel)
}

unsafe fn checked_utf8<'a>(ptr: *const u8, len: usize) -> Result<&'a str, i32> {
    if ptr.is_null() || len == 0 || len > 16 * 1024 {
        return Err(-1);
    }
    core::str::from_utf8(unsafe { core::slice::from_raw_parts(ptr, len) }).map_err(|_| -1)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_input_cursor_pos(
    cursor_id: u32,
    out_x: *mut i32,
    out_y: *mut i32,
) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return guest_input_cursor_pos(cursor_id, out_x, out_y);
    }
    if out_x.is_null() || out_y.is_null() || cursor_id == 0 {
        return -1;
    }

    let Some((nx, ny)) = crate::r::cursor::cursor_pos(cursor_id) else {
        return 1;
    };

    let (w, h) = cursor_viewport_dimensions();
    let w1 = w.saturating_sub(1) as f64;
    let h1 = h.saturating_sub(1) as f64;

    unsafe {
        *out_x = libm::round(nx * w1) as i32;
        *out_y = libm::round(ny * h1) as i32;
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_input_pop_keyboard_output(
    out: *mut crate::r::keyboard::TrueosKeyboardOutputEvent,
) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return guest_input_pop_keyboard_output(out);
    }
    unsafe { input_pop_keyboard_output(out) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_input_read_keyboard_output_since(
    read_seq: u64,
    out: *mut crate::r::keyboard::TrueosKeyboardOutputEvent,
    out_cap: u32,
    out_next_seq: *mut u64,
    out_dropped: *mut u32,
) -> u32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return guest_input_read_keyboard_output_since(
            read_seq,
            out,
            out_cap,
            out_next_seq,
            out_dropped,
        );
    }
    unsafe { input_read_keyboard_output_since(read_seq, out, out_cap, out_next_seq, out_dropped) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_input_cursor_buttons(
    cursor_id: u32,
    out_buttons_down: *mut u32,
) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return guest_input_cursor_buttons(cursor_id, out_buttons_down);
    }
    unsafe { input_cursor_buttons(cursor_id, out_buttons_down) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_input_pop_cursor_event(
    out: *mut crate::usb2::hid::TrueosHidCursorEvent,
) -> i32 {
    unsafe { input_pop_cursor_event(out) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_input_read_cursor_events_since(
    read_seq: u64,
    out: *mut crate::usb2::hid::TrueosHidCursorEvent,
    out_cap: u32,
    out_next_seq: *mut u64,
    out_dropped: *mut u32,
) -> u32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return guest_input_read_cursor_events_since(
            read_seq,
            out,
            out_cap,
            out_next_seq,
            out_dropped,
        );
    }
    unsafe { input_read_cursor_events_since(read_seq, out, out_cap, out_next_seq, out_dropped) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_input_write_cursor(
    slot_id: u32,
    x: i32,
    y: i32,
    buttons_down: u32,
    wheel: i32,
    flags: u32,
) -> i32 {
    input_write_cursor_event(slot_id, x, y, buttons_down, wheel, flags)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_mouse_motion_cursor_request(
    label_ptr: *const u8,
    label_len: usize,
    out_cursor: *mut v::vinput::MouseMotionCursorInfo,
) -> i32 {
    if out_cursor.is_null() {
        return -1;
    }
    let label = match unsafe { checked_utf8(label_ptr, label_len) } {
        Ok(label) => label,
        Err(error) => return error,
    };
    match request_cursor(mouse_motion_principal(), label, None) {
        Ok(cursor) => {
            unsafe {
                *out_cursor = v::vinput::MouseMotionCursorInfo {
                    handle: cursor.handle,
                    slot_id: cursor.slot_id,
                    reserved: 0,
                };
            }
            0
        }
        Err(error) => error.code(),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_mouse_motion_cursor_release(handle: u64) -> i32 {
    release_cursor(mouse_motion_principal(), handle)
        .map(|()| 0)
        .unwrap_or_else(|error| error.code())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_mouse_motion_submit(
    handle: u64,
    command: *const v::vinput::MouseMotionCommand,
) -> i32 {
    if command.is_null() {
        return -1;
    }
    submit_command(mouse_motion_principal(), handle, unsafe { *command }.into())
        .map(|()| 0)
        .unwrap_or_else(|error| error.code())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_mouse_motion_submit_json(
    handle: u64,
    json_ptr: *const u8,
    json_len: usize,
) -> i32 {
    if json_ptr.is_null() || json_len == 0 || json_len > 16 * 1024 {
        return -1;
    }
    let bytes = unsafe { core::slice::from_raw_parts(json_ptr, json_len) };
    submit_json(mouse_motion_principal(), handle, bytes)
        .map(|count| count.min(i32::MAX as usize) as i32)
        .unwrap_or_else(|error| error.code())
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_mouse_motion_cursor_idle(handle: u64) -> i32 {
    crate::r::mouse_motion_service::cursor_is_idle(mouse_motion_principal(), handle)
        .map(i32::from)
        .unwrap_or_else(|error| error.code())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_keyboard_control_request(
    label_ptr: *const u8,
    label_len: usize,
    out_keyboard: *mut v::vinput::KeyboardControlDeviceInfo,
) -> i32 {
    if out_keyboard.is_null() {
        return -1;
    }
    let label = match unsafe { checked_utf8(label_ptr, label_len) } {
        Ok(label) => label,
        Err(error) => return error,
    };
    match request_keyboard(keyboard_control_principal(), label) {
        Ok(keyboard) => {
            unsafe {
                *out_keyboard = v::vinput::KeyboardControlDeviceInfo {
                    handle: keyboard.handle,
                    slot_id: keyboard.slot_id,
                    reserved: 0,
                };
            }
            0
        }
        Err(error) => error.code(),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_keyboard_control_release(handle: u64) -> i32 {
    release_keyboard(keyboard_control_principal(), handle)
        .map(|()| 0)
        .unwrap_or_else(|error| error.code())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_keyboard_control_submit(
    handle: u64,
    command: *const v::vinput::KeyboardControlCommand,
) -> i32 {
    if command.is_null() {
        return -1;
    }
    submit_keyboard_command(keyboard_control_principal(), handle, unsafe { *command }.into())
        .map(|()| 0)
        .unwrap_or_else(|error| error.code())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_keyboard_control_submit_text(
    handle: u64,
    text_ptr: *const u8,
    text_len: usize,
    interval_ms: u32,
    flags: u32,
) -> i32 {
    let text = match unsafe { checked_utf8(text_ptr, text_len) } {
        Ok(text) => text,
        Err(error) => return error,
    };
    submit_keyboard_text(keyboard_control_principal(), handle, text, interval_ms, flags & 1 != 0)
        .map(|count| count.min(i32::MAX as usize) as i32)
        .unwrap_or_else(|error| error.code())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_keyboard_control_submit_json(
    handle: u64,
    json_ptr: *const u8,
    json_len: usize,
) -> i32 {
    if json_ptr.is_null() || json_len == 0 || json_len > 16 * 1024 {
        return -1;
    }
    let bytes = unsafe { core::slice::from_raw_parts(json_ptr, json_len) };
    submit_keyboard_json(keyboard_control_principal(), handle, bytes)
        .map(|count| count.min(i32::MAX as usize) as i32)
        .unwrap_or_else(|error| error.code())
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_keyboard_control_idle(handle: u64) -> i32 {
    keyboard_is_idle(keyboard_control_principal(), handle)
        .map(i32::from)
        .unwrap_or_else(|error| error.code())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_gamepad_control_request(
    label_ptr: *const u8,
    label_len: usize,
    out_gamepad: *mut v::vinput::GamepadControlDeviceInfo,
) -> i32 {
    if out_gamepad.is_null() {
        return -1;
    }
    let label = match unsafe { checked_utf8(label_ptr, label_len) } {
        Ok(label) => label,
        Err(error) => return error,
    };
    match request_gamepad(gamepad_control_principal(), label) {
        Ok(gamepad) => {
            unsafe {
                *out_gamepad = v::vinput::GamepadControlDeviceInfo {
                    handle: gamepad.handle,
                    slot_id: gamepad.slot_id,
                    reserved: 0,
                };
            }
            0
        }
        Err(error) => error.code(),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_gamepad_control_release(handle: u64) -> i32 {
    release_gamepad(gamepad_control_principal(), handle)
        .map(|()| 0)
        .unwrap_or_else(|error| error.code())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_gamepad_control_submit(
    handle: u64,
    command: *const v::vinput::GamepadControlCommand,
) -> i32 {
    if command.is_null() {
        return -1;
    }
    submit_gamepad_command(gamepad_control_principal(), handle, unsafe { *command }.into())
        .map(|()| 0)
        .unwrap_or_else(|error| error.code())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_gamepad_control_submit_json(
    handle: u64,
    json_ptr: *const u8,
    json_len: usize,
) -> i32 {
    if json_ptr.is_null() || json_len == 0 || json_len > 16 * 1024 {
        return -1;
    }
    let bytes = unsafe { core::slice::from_raw_parts(json_ptr, json_len) };
    submit_gamepad_json(gamepad_control_principal(), handle, bytes)
        .map(|count| count.min(i32::MAX as usize) as i32)
        .unwrap_or_else(|error| error.code())
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_gamepad_control_idle(handle: u64) -> i32 {
    gamepad_is_idle(gamepad_control_principal(), handle)
        .map(i32::from)
        .unwrap_or_else(|error| error.code())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_gamepad_control_snapshot(
    handle: u64,
    out_snapshot: *mut v::vinput::GamepadControlSnapshot,
) -> i32 {
    if out_snapshot.is_null() {
        return -1;
    }
    match gamepad_snapshot(gamepad_control_principal(), handle) {
        Ok(snapshot) => {
            unsafe {
                *out_snapshot = v::vinput::GamepadControlSnapshot {
                    slot_id: snapshot.slot_id,
                    sequence: snapshot.sequence,
                    buttons_down: snapshot.buttons_down,
                    reserved0: snapshot.reserved0,
                    left_x: snapshot.left_x,
                    left_y: snapshot.left_y,
                    right_x: snapshot.right_x,
                    right_y: snapshot.right_y,
                    left_trigger: snapshot.left_trigger,
                    right_trigger: snapshot.right_trigger,
                };
            }
            0
        }
        Err(error) => error.code(),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_input_combo_request(
    source_kind: u8,
    requested_color: i32,
    label_ptr: *const u8,
    label_len: usize,
    out_combo: *mut v::vinput::TrueosHidHutCombo,
) -> i32 {
    if out_combo.is_null() {
        return -1;
    }
    let label = match unsafe { checked_utf8(label_ptr, label_len) } {
        Ok(label) if !label.is_empty() => label,
        _ => return -1,
    };
    let color = if requested_color == v::vinput::INPUT_COMBO_COLOR_AUTO {
        None
    } else if (0..i32::from(v::vinput::InputComboColor::COUNT)).contains(&requested_color) {
        Some(requested_color as u8)
    } else {
        return -1;
    };
    let Some(combo) = crate::usb2::hid::hut::request_combo(
        input_combo_source_kind(source_kind),
        label,
        color,
    ) else {
        return -4;
    };
    unsafe {
        *out_combo = input_combo_info(&combo);
    }
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_input_combo_set_color(combo_id: u32, color_id: u8) -> i32 {
    if crate::usb2::hid::hut::set_combo_color(combo_id, color_id) {
        0
    } else {
        -1
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_input_combo_bind_mouse(
    combo_id: u32,
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
) -> i32 {
    if crate::usb2::hid::hut::bind_combo_mouse(
        combo_id,
        controller_id,
        slot_id,
        ep_target,
    ) {
        0
    } else {
        -1
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_input_combo_bind_keyboard(
    combo_id: u32,
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
) -> i32 {
    if crate::usb2::hid::hut::bind_combo_keyboard(
        combo_id,
        controller_id,
        slot_id,
        ep_target,
    ) {
        0
    } else {
        -1
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_input_combo_bind_tablet(
    combo_id: u32,
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
) -> i32 {
    if crate::usb2::hid::hut::bind_combo_tablet(
        combo_id,
        controller_id,
        slot_id,
        ep_target,
    ) {
        0
    } else {
        -1
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_input_combo_bind_gamepad(
    combo_id: u32,
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
) -> i32 {
    if crate::usb2::hid::hut::bind_combo_gamepad(
        combo_id,
        controller_id,
        slot_id,
        ep_target,
    ) {
        0
    } else {
        -1
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_input_combo_remove(combo_id: u32) -> i32 {
    if crate::usb2::hid::hut::remove_combo(combo_id) {
        0
    } else {
        -3
    }
}
