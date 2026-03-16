extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;

pub use crate::vcabi::{TrueosHidCursorEvent, TrueosHidKeyboardSample, TrueosMouseState};
use crate::vcabi;

#[inline]
pub fn mouse_poll() -> Option<TrueosMouseState> {
    let mut out = TrueosMouseState::default();
    let rc = unsafe { vcabi::trueos_cabi_mouse_poll(&mut out) };
    if rc == 0 { Some(out) } else { None }
}

#[inline]
pub fn qjs_mouse_pop() -> Option<TrueosMouseState> {
    let mut out = TrueosMouseState::default();
    let rc = unsafe { vcabi::trueos_cabi_qjs_mouse_pop(&mut out) };
    if rc == 0 { Some(out) } else { None }
}

#[inline]
pub fn pop_mouse_delta() -> Option<(u8, i8, i8, i8)> {
    let mut buttons = 0u8;
    let mut dx = 0i8;
    let mut dy = 0i8;
    let mut wheel = 0i8;
    let rc = unsafe {
        vcabi::trueos_cabi_input_pop_mouse(&mut buttons, &mut dx, &mut dy, &mut wheel)
    };
    if rc == 0 {
        Some((buttons, dx, dy, wheel))
    } else {
        None
    }
}

#[inline]
pub fn cursor_pos(cursor_id: u32) -> Result<(i32, i32), i32> {
    let mut x = 0i32;
    let mut y = 0i32;
    let rc = unsafe { vcabi::trueos_cabi_input_cursor_pos(cursor_id, &mut x, &mut y) };
    if rc != 0 {
        return Err(rc);
    }
    Ok((x, y))
}

#[inline]
pub fn cursor_buttons(cursor_id: u32) -> Result<u32, i32> {
    let mut buttons = 0u32;
    let rc = unsafe { vcabi::trueos_cabi_input_cursor_buttons(cursor_id, &mut buttons) };
    if rc != 0 {
        return Err(rc);
    }
    Ok(buttons)
}

#[inline]
pub fn read_cursor_events_since(
    read_seq: u64,
    out_cap: u32,
) -> Result<(Vec<TrueosHidCursorEvent>, u64, u32), i32> {
    let mut events = vec![TrueosHidCursorEvent::default(); out_cap as usize];
    let mut next_seq = read_seq;
    let mut dropped = 0u32;
    let got = unsafe {
        vcabi::trueos_cabi_input_read_cursor_events_since(
            read_seq,
            events.as_mut_ptr(),
            out_cap,
            &mut next_seq,
            &mut dropped,
        )
    };
    if got == 0 && out_cap != 0 && dropped == 0 && next_seq == read_seq {
        return Ok((Vec::new(), next_seq, dropped));
    }
    events.truncate(got as usize);
    Ok((events, next_seq, dropped))
}

#[inline]
pub fn write_cursor(
    slot_id: u32,
    x: i32,
    y: i32,
    buttons_down: u32,
    wheel: i32,
    flags: u32,
) -> Result<(), i32> {
    let rc = unsafe {
        vcabi::trueos_cabi_input_write_cursor(slot_id, x, y, buttons_down, wheel, flags)
    };
    if rc != 0 {
        return Err(rc);
    }
    Ok(())
}

#[inline]
pub fn hid_keyboard_read(
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
    out_cap: u32,
) -> (Vec<TrueosHidKeyboardSample>, u32) {
    let mut samples = vec![TrueosHidKeyboardSample::default(); out_cap as usize];
    let mut dropped = 0u32;
    let got = unsafe {
        vcabi::trueos_cabi_hid_keyboard_read(
            controller_id,
            slot_id,
            ep_target,
            samples.as_mut_ptr(),
            out_cap,
            &mut dropped,
        )
    };
    samples.truncate(got as usize);
    (samples, dropped)
}

#[inline]
pub fn write_keyboard_text(slot_id: u32, bytes: &[u8], flags: u32) -> Result<(), i32> {
    if slot_id == 0 || bytes.is_empty() {
        return Err(-1);
    }
    let rc = unsafe {
        vcabi::trueos_cabi_input_write_keyboard_text(slot_id, bytes.as_ptr(), bytes.len(), flags)
    };
    if rc != 0 {
        return Err(rc);
    }
    Ok(())
}

#[inline]
pub fn write_keyboard_key(
    slot_id: u32,
    codepoint: u32,
    key_code: u32,
    modifiers: u32,
    flags: u32,
) -> Result<(), i32> {
    if slot_id == 0 {
        return Err(-1);
    }
    let rc = unsafe {
        vcabi::trueos_cabi_input_write_keyboard_key(slot_id, codepoint, key_code, modifiers, flags)
    };
    if rc != 0 {
        return Err(rc);
    }
    Ok(())
}
