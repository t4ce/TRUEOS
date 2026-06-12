extern crate alloc;

use core::sync::atomic::{AtomicU32, Ordering};

static VM_CURSOR_WRITE_REJECT_COUNT: AtomicU32 = AtomicU32::new(0);

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
    if let Some(vm_id) = crate::hv::current_guest_execution_context_vm_id() {
        let count = VM_CURSOR_WRITE_REJECT_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
        if count <= 8 || count.is_multiple_of(64) {
            crate::log!(
                "WARNING input-cursor: rejected vm cursor write vm={} slot={} x={} y={} buttons=0x{:X} wheel={} flags=0x{:X} count={}\n",
                vm_id,
                slot_id,
                x_px,
                y_px,
                buttons_down,
                wheel,
                flags,
                count
            );
        }
        return -1;
    }

    let (w, h) = cursor_viewport_dimensions();
    let max_x = w.saturating_sub(1) as i32;
    let max_y = h.saturating_sub(1) as i32;
    let clamped_x = x_px.clamp(0, max_x.max(0));
    let clamped_y = y_px.clamp(0, max_y.max(0));
    let w1 = (w.saturating_sub(1)).max(1) as f64;
    let h1 = (h.saturating_sub(1)).max(1) as f64;
    let nx = (clamped_x as f64) / w1;
    let ny = (clamped_y as f64) / h1;
    let wheel_i16 = wheel.clamp(i16::MIN as i32, i16::MAX as i32) as i16;

    crate::usb2::hid::inject_virtual_cursor_event(slot_id, nx, ny, buttons_down, wheel_i16, flags);
    0
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
