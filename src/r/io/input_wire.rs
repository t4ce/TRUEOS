//! Explicit little-endian encoders for input records crossing the host/guest ABI.
//! Explicit reserved fields are preserved; implicit C padding is zeroed.

use crate::r::keyboard::TrueosKeyboardOutputEvent;
use crate::usb2::hid::TrueosHidCursorEvent;
use crate::usb3::hid::midi::MidiInputEventV1;
use crate::usb3::hid::hut::{TrueosHidHutKeyboardState, TrueosHidHutMouseState, TrueosHidHutTabletState};
use v::vinput::{KeyboardControlCommand, MouseMotionCommand, TrueosInputCombo};

pub(crate) const CURSOR_EVENT_LEN: usize = 56;
pub(crate) const KEYBOARD_OUTPUT_LEN: usize = 44;
pub(crate) const MIDI_INPUT_V1_LEN: usize = 24;
pub(crate) const MOUSE_COMMAND_LEN: usize = 44;
pub(crate) const KEYBOARD_COMMAND_LEN: usize = 16;
pub(crate) const HUT_KEYBOARD_LEN: usize = 96;
pub(crate) const HUT_MOUSE_LEN: usize = 72;
pub(crate) const HUT_TABLET_LEN: usize = 80;
pub(crate) const INPUT_COMBO_LEN: usize = 88;
pub(crate) const HUT_SNAPSHOT_LIMIT: u32 = 32;
pub(crate) const HUT_SNAPSHOT_KEYBOARDS: u32 = 1;
pub(crate) const HUT_SNAPSHOT_COMBOS: u32 = 2;
pub(crate) const HUT_SNAPSHOT_MICE: u32 = 3;
pub(crate) const HUT_SNAPSHOT_TABLETS: u32 = 4;

macro_rules! field {
    ($ty:ty, $name:ident, $offset:expr, $width:expr, $field_ty:ty) => {
        assert!(core::mem::offset_of!($ty, $name) == $offset);
        const _: fn(&$ty) -> &$field_ty = |value| &value.$name;
        assert!(core::mem::size_of::<$field_ty>() == $width);
    };
}

const _: () = {
    assert!(core::mem::size_of::<TrueosHidHutTabletState>() == HUT_TABLET_LEN);
    field!(TrueosHidHutTabletState, controller_id, 0, 4, u32);
    field!(TrueosHidHutTabletState, slot_id, 4, 4, u32);
    field!(TrueosHidHutTabletState, ep_target, 8, 4, u32);
    field!(TrueosHidHutTabletState, x_raw, 12, 2, u16);
    field!(TrueosHidHutTabletState, y_raw, 14, 2, u16);
    field!(TrueosHidHutTabletState, buttons_down, 16, 4, u32);
    field!(TrueosHidHutTabletState, report_id, 20, 1, u8);
    field!(TrueosHidHutTabletState, source_kind, 21, 1, u8);
    field!(TrueosHidHutTabletState, virtual_device, 22, 1, u8);
    field!(TrueosHidHutTabletState, source_tag_len, 23, 1, u8);
    field!(TrueosHidHutTabletState, combo_id, 24, 4, u32);
    field!(TrueosHidHutTabletState, source_tag, 28, 32, [u8; 32]);
    field!(TrueosHidHutTabletState, x, 64, 8, f64);
    field!(TrueosHidHutTabletState, y, 72, 8, f64);
    assert!(core::mem::size_of::<TrueosHidHutMouseState>() == HUT_MOUSE_LEN);
    field!(TrueosHidHutMouseState, controller_id, 0, 4, u32);
    field!(TrueosHidHutMouseState, slot_id, 4, 4, u32);
    field!(TrueosHidHutMouseState, ep_target, 8, 4, u32);
    field!(TrueosHidHutMouseState, buttons_down, 12, 4, u32);
    field!(TrueosHidHutMouseState, combo_id, 16, 4, u32);
    field!(TrueosHidHutMouseState, source_kind, 20, 1, u8);
    field!(TrueosHidHutMouseState, virtual_device, 21, 1, u8);
    field!(TrueosHidHutMouseState, source_tag_len, 22, 1, u8);
    field!(TrueosHidHutMouseState, reserved0, 23, 1, u8);
    field!(TrueosHidHutMouseState, source_tag, 24, 32, [u8; 32]);
    field!(TrueosHidHutMouseState, x, 56, 8, f64);
    field!(TrueosHidHutMouseState, y, 64, 8, f64);
    assert!(core::mem::size_of::<TrueosHidHutKeyboardState>() == HUT_KEYBOARD_LEN);
    field!(TrueosHidHutKeyboardState, controller_id, 0, 4, u32);
    field!(TrueosHidHutKeyboardState, slot_id, 4, 4, u32);
    field!(TrueosHidHutKeyboardState, ep_target, 8, 4, u32);
    field!(TrueosHidHutKeyboardState, combo_id, 12, 4, u32);
    field!(TrueosHidHutKeyboardState, modifiers, 16, 1, u8);
    field!(TrueosHidHutKeyboardState, source_kind, 17, 1, u8);
    field!(TrueosHidHutKeyboardState, virtual_device, 18, 1, u8);
    field!(TrueosHidHutKeyboardState, source_tag_len, 19, 1, u8);
    field!(TrueosHidHutKeyboardState, keys, 20, 6, [u8; 6]);
    field!(TrueosHidHutKeyboardState, ascii, 26, 6, [u8; 6]);
    field!(TrueosHidHutKeyboardState, key_down_bits, 32, 32, [u32; 8]);
    field!(TrueosHidHutKeyboardState, source_tag, 64, 32, [u8; 32]);
    assert!(core::mem::size_of::<TrueosInputCombo>() == INPUT_COMBO_LEN);
    field!(TrueosInputCombo, combo_id, 0, 4, u32);
    field!(TrueosInputCombo, source_kind, 4, 1, u8);
    field!(TrueosInputCombo, source_tag_len, 5, 1, u8);
    field!(TrueosInputCombo, color_id, 6, 1, u8);
    field!(TrueosInputCombo, flags, 7, 1, u8);
    field!(TrueosInputCombo, source_tag, 8, 32, [u8; 32]);
    field!(TrueosInputCombo, mouse_controller_id, 40, 4, u32);
    field!(TrueosInputCombo, mouse_slot_id, 44, 4, u32);
    field!(TrueosInputCombo, mouse_ep_target, 48, 4, u32);
    field!(TrueosInputCombo, keyboard_controller_id, 52, 4, u32);
    field!(TrueosInputCombo, keyboard_slot_id, 56, 4, u32);
    field!(TrueosInputCombo, keyboard_ep_target, 60, 4, u32);
    field!(TrueosInputCombo, tablet_controller_id, 64, 4, u32);
    field!(TrueosInputCombo, tablet_slot_id, 68, 4, u32);
    field!(TrueosInputCombo, tablet_ep_target, 72, 4, u32);
    field!(TrueosInputCombo, gamepad_controller_id, 76, 4, u32);
    field!(TrueosInputCombo, gamepad_slot_id, 80, 4, u32);
    field!(TrueosInputCombo, gamepad_ep_target, 84, 4, u32);

    assert!(core::mem::size_of::<TrueosHidCursorEvent>() == CURSOR_EVENT_LEN);
    field!(TrueosHidCursorEvent, t_ms, 0, 4, u32);
    field!(TrueosHidCursorEvent, seq, 4, 4, u32);
    field!(TrueosHidCursorEvent, controller_id, 8, 4, u32);
    field!(TrueosHidCursorEvent, slot_id, 12, 4, u32);
    field!(TrueosHidCursorEvent, ep_target, 16, 4, u32);
    field!(TrueosHidCursorEvent, hid_kind, 20, 1, u8);
    field!(TrueosHidCursorEvent, reserved0, 21, 1, u8);
    field!(TrueosHidCursorEvent, reserved1, 22, 2, u16);
    field!(TrueosHidCursorEvent, buttons_down, 24, 4, u32);
    field!(TrueosHidCursorEvent, wheel, 28, 2, i16);
    field!(TrueosHidCursorEvent, reserved2, 30, 2, u16);
    field!(TrueosHidCursorEvent, x, 32, 8, f64);
    field!(TrueosHidCursorEvent, y, 40, 8, f64);
    field!(TrueosHidCursorEvent, flags, 48, 4, u32);

    assert!(core::mem::size_of::<TrueosKeyboardOutputEvent>() == KEYBOARD_OUTPUT_LEN);
    field!(TrueosKeyboardOutputEvent, t_ms, 0, 4, u32);
    field!(TrueosKeyboardOutputEvent, seq, 4, 4, u32);
    field!(TrueosKeyboardOutputEvent, device_seq, 8, 4, u32);
    field!(TrueosKeyboardOutputEvent, controller_id, 12, 4, u32);
    field!(TrueosKeyboardOutputEvent, slot_id, 16, 4, u32);
    field!(TrueosKeyboardOutputEvent, ep_target, 20, 4, u32);
    field!(TrueosKeyboardOutputEvent, modifiers, 24, 1, u8);
    field!(TrueosKeyboardOutputEvent, kind, 25, 1, u8);
    field!(TrueosKeyboardOutputEvent, utf8_len, 26, 1, u8);
    field!(TrueosKeyboardOutputEvent, reserved0, 27, 1, u8);
    field!(TrueosKeyboardOutputEvent, key_code, 28, 2, u16);
    field!(TrueosKeyboardOutputEvent, reserved1, 30, 2, u16);
    field!(TrueosKeyboardOutputEvent, codepoint, 32, 4, u32);
    field!(TrueosKeyboardOutputEvent, utf8, 36, 4, [u8; 4]);
    field!(TrueosKeyboardOutputEvent, flags, 40, 4, u32);

    assert!(core::mem::size_of::<MidiInputEventV1>() == MIDI_INPUT_V1_LEN);
    field!(MidiInputEventV1, seq, 0, 8, u64);
    field!(MidiInputEventV1, controller_id, 8, 4, u32);
    field!(MidiInputEventV1, slot_id, 12, 4, u32);
    field!(MidiInputEventV1, channel, 16, 1, u8);
    field!(MidiInputEventV1, note, 17, 1, u8);
    field!(MidiInputEventV1, velocity, 18, 1, u8);
    field!(MidiInputEventV1, gate, 19, 1, u8);

    assert!(core::mem::size_of::<MouseMotionCommand>() == MOUSE_COMMAND_LEN);
    field!(MouseMotionCommand, opcode, 0, 1, u8);
    field!(MouseMotionCommand, path, 1, 1, u8);
    field!(MouseMotionCommand, easing, 2, 1, u8);
    field!(MouseMotionCommand, flags, 3, 1, u8);
    field!(MouseMotionCommand, duration_ms, 4, 4, u32);
    field!(MouseMotionCommand, x, 8, 4, i32);
    field!(MouseMotionCommand, y, 12, 4, i32);
    field!(MouseMotionCommand, control1_x, 16, 4, i32);
    field!(MouseMotionCommand, control1_y, 20, 4, i32);
    field!(MouseMotionCommand, control2_x, 24, 4, i32);
    field!(MouseMotionCommand, control2_y, 28, 4, i32);
    field!(MouseMotionCommand, buttons_set, 32, 4, u32);
    field!(MouseMotionCommand, buttons_clear, 36, 4, u32);
    field!(MouseMotionCommand, wheel, 40, 2, i16);
    field!(MouseMotionCommand, reserved, 42, 2, u16);

    assert!(core::mem::size_of::<KeyboardControlCommand>() == KEYBOARD_COMMAND_LEN);
    field!(KeyboardControlCommand, opcode, 0, 1, u8);
    field!(KeyboardControlCommand, flags, 1, 1, u8);
    field!(KeyboardControlCommand, modifiers, 2, 1, u8);
    field!(KeyboardControlCommand, reserved0, 3, 1, u8);
    field!(KeyboardControlCommand, duration_ms, 4, 4, u32);
    field!(KeyboardControlCommand, codepoint, 8, 4, u32);
    field!(KeyboardControlCommand, key_code, 12, 2, u16);
    field!(KeyboardControlCommand, reserved1, 14, 2, u16);
};

fn begin(out: &mut [u8], size: usize) -> bool {
    if out.len() < size {
        return false;
    }
    out[..size].fill(0);
    true
}
fn put<const N: usize>(out: &mut [u8], at: usize, value: [u8; N]) {
    out[at..at + N].copy_from_slice(&value);
}

pub(crate) fn hut_snapshot_record_len(kind: u32) -> Option<usize> {
    match kind {
        HUT_SNAPSHOT_KEYBOARDS => Some(HUT_KEYBOARD_LEN),
        HUT_SNAPSHOT_COMBOS => Some(INPUT_COMBO_LEN),
        HUT_SNAPSHOT_MICE => Some(HUT_MOUSE_LEN),
        HUT_SNAPSHOT_TABLETS => Some(HUT_TABLET_LEN),
        _ => None,
    }
}

/// Count probes have no payload; reads must contain exactly the admitted count.
pub(crate) fn hut_snapshot_response_valid(
    kind: u32,
    requested_cap: u32,
    count: u64,
    reported_len: usize,
    available_len: usize,
) -> bool {
    let Some(record_len) = hut_snapshot_record_len(kind) else { return false };
    if requested_cap > HUT_SNAPSHOT_LIMIT || count > u64::from(HUT_SNAPSHOT_LIMIT) {
        return false;
    }
    let expected_len = if requested_cap == 0 {
        0
    } else {
        if count > u64::from(requested_cap) { return false; }
        count as usize * record_len
    };
    reported_len == expected_len && reported_len <= available_len
}

pub(crate) fn hut_tablet(out: &mut [u8], tablet: &TrueosHidHutTabletState) -> bool {
    if !begin(out, HUT_TABLET_LEN) { return false; }
    for (at, value) in [(0, tablet.controller_id), (4, tablet.slot_id), (8, tablet.ep_target), (16, tablet.buttons_down), (24, tablet.combo_id)] {
        put(out, at, value.to_le_bytes());
    }
    put(out, 12, tablet.x_raw.to_le_bytes());
    put(out, 14, tablet.y_raw.to_le_bytes());
    out[20..24].copy_from_slice(&[tablet.report_id, tablet.source_kind, tablet.virtual_device, tablet.source_tag_len]);
    put(out, 28, tablet.source_tag);
    put(out, 64, tablet.x.to_le_bytes());
    put(out, 72, tablet.y.to_le_bytes());
    true
}

pub(crate) fn decode_hut_tablet(bytes: &[u8]) -> Option<TrueosHidHutTabletState> {
    if bytes.len() != HUT_TABLET_LEN { return None; }
    let word = |at| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    Some(TrueosHidHutTabletState {
        controller_id: word(0), slot_id: word(4), ep_target: word(8), buttons_down: word(16), combo_id: word(24),
        x_raw: u16::from_le_bytes(bytes[12..14].try_into().unwrap()), y_raw: u16::from_le_bytes(bytes[14..16].try_into().unwrap()),
        report_id: bytes[20], source_kind: bytes[21], virtual_device: bytes[22], source_tag_len: bytes[23],
        source_tag: bytes[28..60].try_into().unwrap(),
        x: f64::from_le_bytes(bytes[64..72].try_into().unwrap()),
        y: f64::from_le_bytes(bytes[72..80].try_into().unwrap()),
    })
}

pub(crate) fn hut_mouse(out: &mut [u8], mouse: &TrueosHidHutMouseState) -> bool {
    if !begin(out, HUT_MOUSE_LEN) { return false; }
    for (index, value) in [mouse.controller_id, mouse.slot_id, mouse.ep_target, mouse.buttons_down, mouse.combo_id].into_iter().enumerate() {
        put(out, index * 4, value.to_le_bytes());
    }
    out[20..24].copy_from_slice(&[mouse.source_kind, mouse.virtual_device, mouse.source_tag_len, mouse.reserved0]);
    put(out, 24, mouse.source_tag);
    put(out, 56, mouse.x.to_le_bytes());
    put(out, 64, mouse.y.to_le_bytes());
    true
}

pub(crate) fn decode_hut_mouse(bytes: &[u8]) -> Option<TrueosHidHutMouseState> {
    if bytes.len() != HUT_MOUSE_LEN { return None; }
    let word = |at| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    Some(TrueosHidHutMouseState {
        controller_id: word(0), slot_id: word(4), ep_target: word(8), buttons_down: word(12), combo_id: word(16),
        source_kind: bytes[20], virtual_device: bytes[21], source_tag_len: bytes[22], reserved0: bytes[23],
        source_tag: bytes[24..56].try_into().unwrap(),
        x: f64::from_le_bytes(bytes[56..64].try_into().unwrap()),
        y: f64::from_le_bytes(bytes[64..72].try_into().unwrap()),
    })
}

pub(crate) fn hut_keyboard(out: &mut [u8], keyboard: &TrueosHidHutKeyboardState) -> bool {
    if !begin(out, HUT_KEYBOARD_LEN) { return false; }
    for (index, value) in [keyboard.controller_id, keyboard.slot_id, keyboard.ep_target, keyboard.combo_id].into_iter().enumerate() {
        put(out, index * 4, value.to_le_bytes());
    }
    out[16..20].copy_from_slice(&[keyboard.modifiers, keyboard.source_kind, keyboard.virtual_device, keyboard.source_tag_len]);
    put(out, 20, keyboard.keys);
    put(out, 26, keyboard.ascii);
    for (index, value) in keyboard.key_down_bits.into_iter().enumerate() {
        put(out, 32 + index * 4, value.to_le_bytes());
    }
    put(out, 64, keyboard.source_tag);
    true
}

pub(crate) fn decode_hut_keyboard(bytes: &[u8]) -> Option<TrueosHidHutKeyboardState> {
    if bytes.len() != HUT_KEYBOARD_LEN { return None; }
    let word = |at| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    Some(TrueosHidHutKeyboardState {
        controller_id: word(0), slot_id: word(4), ep_target: word(8), combo_id: word(12),
        modifiers: bytes[16], source_kind: bytes[17], virtual_device: bytes[18], source_tag_len: bytes[19],
        keys: bytes[20..26].try_into().unwrap(), ascii: bytes[26..32].try_into().unwrap(),
        key_down_bits: core::array::from_fn(|index| word(32 + index * 4)),
        source_tag: bytes[64..96].try_into().unwrap(),
    })
}

pub(crate) fn input_combo(out: &mut [u8], combo: &TrueosInputCombo) -> bool {
    if !begin(out, INPUT_COMBO_LEN) { return false; }
    put(out, 0, combo.combo_id.to_le_bytes());
    out[4..8].copy_from_slice(&[combo.source_kind, combo.source_tag_len, combo.color_id, combo.flags]);
    put(out, 8, combo.source_tag);
    for (index, value) in [
        combo.mouse_controller_id, combo.mouse_slot_id, combo.mouse_ep_target,
        combo.keyboard_controller_id, combo.keyboard_slot_id, combo.keyboard_ep_target,
        combo.tablet_controller_id, combo.tablet_slot_id, combo.tablet_ep_target,
        combo.gamepad_controller_id, combo.gamepad_slot_id, combo.gamepad_ep_target,
    ].into_iter().enumerate() {
        put(out, 40 + index * 4, value.to_le_bytes());
    }
    true
}

pub(crate) fn decode_input_combo(bytes: &[u8]) -> Option<TrueosInputCombo> {
    if bytes.len() != INPUT_COMBO_LEN { return None; }
    let word = |at| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    Some(TrueosInputCombo {
        combo_id: word(0), source_kind: bytes[4], source_tag_len: bytes[5], color_id: bytes[6], flags: bytes[7],
        source_tag: bytes[8..40].try_into().unwrap(),
        mouse_controller_id: word(40), mouse_slot_id: word(44), mouse_ep_target: word(48),
        keyboard_controller_id: word(52), keyboard_slot_id: word(56), keyboard_ep_target: word(60),
        tablet_controller_id: word(64), tablet_slot_id: word(68), tablet_ep_target: word(72),
        gamepad_controller_id: word(76), gamepad_slot_id: word(80), gamepad_ep_target: word(84),
    })
}

pub(crate) fn cursor_event(out: &mut [u8], event: &TrueosHidCursorEvent) -> bool {
    if !begin(out, CURSOR_EVENT_LEN) {
        return false;
    }
    put(out, 0, event.t_ms.to_le_bytes());
    put(out, 4, event.seq.to_le_bytes());
    put(out, 8, event.controller_id.to_le_bytes());
    put(out, 12, event.slot_id.to_le_bytes());
    put(out, 16, event.ep_target.to_le_bytes());
    out[20] = event.hid_kind;
    out[21] = event.reserved0;
    put(out, 22, event.reserved1.to_le_bytes());
    put(out, 24, event.buttons_down.to_le_bytes());
    put(out, 28, event.wheel.to_le_bytes());
    put(out, 30, event.reserved2.to_le_bytes());
    put(out, 32, event.x.to_bits().to_le_bytes());
    put(out, 40, event.y.to_bits().to_le_bytes());
    put(out, 48, event.flags.to_le_bytes());
    true
}

pub(crate) fn keyboard_output(out: &mut [u8], event: &TrueosKeyboardOutputEvent) -> bool {
    if !begin(out, KEYBOARD_OUTPUT_LEN) {
        return false;
    }
    put(out, 0, event.t_ms.to_le_bytes());
    put(out, 4, event.seq.to_le_bytes());
    put(out, 8, event.device_seq.to_le_bytes());
    put(out, 12, event.controller_id.to_le_bytes());
    put(out, 16, event.slot_id.to_le_bytes());
    put(out, 20, event.ep_target.to_le_bytes());
    out[24] = event.modifiers;
    out[25] = event.kind;
    out[26] = event.utf8_len;
    out[27] = event.reserved0;
    put(out, 28, event.key_code.to_le_bytes());
    put(out, 30, event.reserved1.to_le_bytes());
    put(out, 32, event.codepoint.to_le_bytes());
    put(out, 36, event.utf8);
    put(out, 40, event.flags.to_le_bytes());
    true
}

pub(crate) fn midi_input_v1(out: &mut [u8], event: &MidiInputEventV1) -> bool {
    if !begin(out, MIDI_INPUT_V1_LEN) {
        return false;
    }
    put(out, 0, event.seq.to_le_bytes());
    put(out, 8, event.controller_id.to_le_bytes());
    put(out, 12, event.slot_id.to_le_bytes());
    out[16] = event.channel;
    out[17] = event.note;
    out[18] = event.velocity;
    out[19] = event.gate;
    true
}

pub(crate) fn mouse_command(out: &mut [u8], command: &MouseMotionCommand) -> bool {
    if !begin(out, MOUSE_COMMAND_LEN) {
        return false;
    }
    out[..4].copy_from_slice(&[command.opcode, command.path, command.easing, command.flags]);
    for (i, v) in [
        command.duration_ms,
        command.x as u32,
        command.y as u32,
        command.control1_x as u32,
        command.control1_y as u32,
        command.control2_x as u32,
        command.control2_y as u32,
        command.buttons_set,
        command.buttons_clear,
    ]
    .into_iter()
    .enumerate()
    {
        put(out, 4 + i * 4, v.to_le_bytes());
    }
    put(out, 40, command.wheel.to_le_bytes());
    put(out, 42, command.reserved.to_le_bytes());
    true
}

pub(crate) fn keyboard_command(out: &mut [u8], command: &KeyboardControlCommand) -> bool {
    if !begin(out, KEYBOARD_COMMAND_LEN) {
        return false;
    }
    out[..4].copy_from_slice(&[
        command.opcode,
        command.flags,
        command.modifiers,
        command.reserved0,
    ]);
    put(out, 4, command.duration_ms.to_le_bytes());
    put(out, 8, command.codepoint.to_le_bytes());
    put(out, 12, command.key_code.to_le_bytes());
    put(out, 14, command.reserved1.to_le_bytes());
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hut_snapshot_records_roundtrip_all_fields_in_little_endian() {
        let tablet = TrueosHidHutTabletState {
            controller_id: 0x01020304, slot_id: 9, ep_target: 3, x_raw: 0x1234, y_raw: 0x5678,
            buttons_down: 0x31415926, report_id: 7, source_kind: 2, virtual_device: 1, source_tag_len: 32,
            combo_id: 0x41424344, source_tag: [0xa5; 32], x: 0.25, y: -0.5,
        };
        let mut bytes = [0xaa; HUT_TABLET_LEN + 2];
        assert!(hut_tablet(&mut bytes, &tablet));
        assert_eq!(&bytes[..4], &[4,3,2,1]);
        assert_eq!(&bytes[12..16], &[0x34,0x12,0x78,0x56]);
        assert_eq!(&bytes[16..24], &[0x26,0x59,0x41,0x31,7,2,1,32]);
        assert_eq!(&bytes[24..28], &[0x44,0x43,0x42,0x41]);
        assert_eq!(&bytes[28..60], &[0xa5; 32]);
        assert_eq!(&bytes[60..64], &[0;4]);
        assert_eq!(&bytes[64..72], &0.25f64.to_le_bytes());
        assert_eq!(&bytes[72..80], &(-0.5f64).to_le_bytes());
        assert_eq!(&bytes[80..], &[0xaa;2]);
        let mut again = [0; HUT_TABLET_LEN];
        assert!(hut_tablet(&mut again, &decode_hut_tablet(&bytes[..80]).unwrap()));
        assert_eq!(&again, &bytes[..80]);
        assert!(decode_hut_tablet(&bytes[..79]).is_none());
        assert!(decode_hut_tablet(&bytes).is_none());
        assert_short_buffer_preserved(HUT_TABLET_LEN, |out| hut_tablet(out, &tablet));

        let mouse = TrueosHidHutMouseState {
            controller_id: 0x0102_0304, slot_id: 0x1112_1314, ep_target: 0x2122_2324,
            buttons_down: 0x3132_3334, combo_id: 0x4142_4344, source_kind: 2,
            virtual_device: 1, source_tag_len: 32, reserved0: 0xa5,
            source_tag: core::array::from_fn(|i| i as u8), x: 0.25, y: -0.5,
        };
        let mut bytes = [0xaa; HUT_MOUSE_LEN + 2];
        assert!(hut_mouse(&mut bytes, &mouse));
        assert_eq!(&bytes[..4], &[4, 3, 2, 1]);
        assert_eq!(&bytes[12..20], &[0x34, 0x33, 0x32, 0x31, 0x44, 0x43, 0x42, 0x41]);
        assert_eq!(&bytes[20..24], &[2, 1, 32, 0xa5]);
        assert_eq!(&bytes[24..56], &mouse.source_tag);
        assert_eq!(&bytes[56..64], &0.25f64.to_le_bytes());
        assert_eq!(&bytes[64..72], &(-0.5f64).to_le_bytes());
        assert_eq!(&bytes[72..], &[0xaa; 2]);
        let decoded = decode_hut_mouse(&bytes[..72]).unwrap();
        let mut encoded_again = [0; HUT_MOUSE_LEN];
        assert!(hut_mouse(&mut encoded_again, &decoded));
        assert_eq!(&encoded_again, &bytes[..72]);
        assert!(decode_hut_mouse(&bytes[..71]).is_none());
        assert!(decode_hut_mouse(&bytes).is_none());
        assert_short_buffer_preserved(HUT_MOUSE_LEN, |out| hut_mouse(out, &mouse));

        let keyboard = TrueosHidHutKeyboardState {
            controller_id: 0x0102_0304, slot_id: 0x1112_1314, ep_target: 0x2122_2324, combo_id: 0x3132_3334,
            modifiers: 0x41, source_kind: 0x42, virtual_device: 1, source_tag_len: 32,
            keys: [1, 2, 3, 4, 5, 6], ascii: [11, 12, 13, 14, 15, 16],
            key_down_bits: core::array::from_fn(|index| 0x5152_5300 + index as u32),
            source_tag: core::array::from_fn(|index| index as u8),
        };
        let mut bytes = [0xaa; HUT_KEYBOARD_LEN + 2];
        assert!(hut_keyboard(&mut bytes, &keyboard));
        assert_eq!(&bytes[..4], &[4, 3, 2, 1]);
        assert_eq!(&bytes[16..32], &[0x41, 0x42, 1, 32, 1, 2, 3, 4, 5, 6, 11, 12, 13, 14, 15, 16]);
        assert_eq!(&bytes[32..36], &[0, 0x53, 0x52, 0x51]);
        assert_eq!(&bytes[64..96], &keyboard.source_tag);
        assert_eq!(&bytes[96..], &[0xaa; 2]);
        let decoded = decode_hut_keyboard(&bytes[..96]).unwrap();
        let mut encoded_again = [0; HUT_KEYBOARD_LEN];
        assert!(hut_keyboard(&mut encoded_again, &decoded));
        assert_eq!(&encoded_again, &bytes[..96]);
        assert!(decode_hut_keyboard(&bytes[..95]).is_none());
        assert!(decode_hut_keyboard(&bytes).is_none());
        assert_short_buffer_preserved(HUT_KEYBOARD_LEN, |out| hut_keyboard(out, &keyboard));

        let combo = TrueosInputCombo {
            combo_id: 0x0102_0304, source_kind: 2, source_tag_len: 32, color_id: 15, flags: 0x80,
            source_tag: core::array::from_fn(|index| 0x80 + index as u8),
            mouse_controller_id: 0x1112_1314, mouse_slot_id: 0x2122_2324, mouse_ep_target: 0x3132_3334,
            keyboard_controller_id: 0x4142_4344, keyboard_slot_id: 0x5152_5354, keyboard_ep_target: 0x6162_6364,
            tablet_controller_id: 0x7172_7374, tablet_slot_id: 0x8182_8384, tablet_ep_target: 0x9192_9394,
            gamepad_controller_id: 0xa1a2_a3a4, gamepad_slot_id: 0xb1b2_b3b4, gamepad_ep_target: 0xc1c2_c3c4,
        };
        let mut bytes = [0xaa; INPUT_COMBO_LEN + 2];
        assert!(input_combo(&mut bytes, &combo));
        assert_eq!(&bytes[..8], &[4, 3, 2, 1, 2, 32, 15, 0x80]);
        assert_eq!(&bytes[8..40], &combo.source_tag);
        assert_eq!(&bytes[40..44], &[0x14, 0x13, 0x12, 0x11]);
        assert_eq!(&bytes[84..88], &[0xc4, 0xc3, 0xc2, 0xc1]);
        assert_eq!(&bytes[88..], &[0xaa; 2]);
        let decoded = decode_input_combo(&bytes[..88]).unwrap();
        let mut encoded_again = [0; INPUT_COMBO_LEN];
        assert!(input_combo(&mut encoded_again, &decoded));
        assert_eq!(&encoded_again, &bytes[..88]);
        assert!(decode_input_combo(&bytes[..87]).is_none());
        assert!(decode_input_combo(&bytes).is_none());
        assert_short_buffer_preserved(INPUT_COMBO_LEN, |out| input_combo(out, &combo));
    }

    #[test]
    fn hut_snapshot_rejects_truncated_oversized_and_inconsistent_responses() {
        for kind in [HUT_SNAPSHOT_KEYBOARDS, HUT_SNAPSHOT_COMBOS, HUT_SNAPSHOT_MICE, HUT_SNAPSHOT_TABLETS] {
            let size = hut_snapshot_record_len(kind).unwrap();
            let bytes = 32 * size;
            assert!(bytes <= 4040);
            for count in [0, 1, 32] {
                assert!(hut_snapshot_response_valid(kind, 0, count, 0, 0));
                assert!(hut_snapshot_response_valid(kind, 32, count, count as usize * size, bytes));
            }
            assert!(!hut_snapshot_response_valid(kind, 0, 1, size, bytes));
            assert!(!hut_snapshot_response_valid(kind, 0, 33, 0, 0));
            assert!(!hut_snapshot_response_valid(kind, 1, 2, 2 * size, bytes));
            assert!(!hut_snapshot_response_valid(kind, 33, 1, size, bytes));
            assert!(!hut_snapshot_response_valid(kind, 32, 32, bytes - 1, bytes));
            assert!(!hut_snapshot_response_valid(kind, 32, 32, bytes + 1, bytes + 1));
            assert!(!hut_snapshot_response_valid(kind, 32, 32, bytes, bytes - 1));
            assert!(!hut_snapshot_response_valid(kind, 32, u64::MAX, bytes, bytes));
        }
        assert!(!hut_snapshot_response_valid(0, 0, 0, 0, 0));
        assert!(!hut_snapshot_response_valid(5, 32, 0, 0, 0));
    }

    #[test]
    fn records_preserve_fields_and_zero_only_padding() {
        let cursor = TrueosHidCursorEvent {
            t_ms: 0x0102_0304,
            seq: 0x0506_0708,
            controller_id: 0x090a_0b0c,
            slot_id: 0x0d0e_0f10,
            ep_target: 0x1112_1314,
            hid_kind: 0x21,
            reserved0: 0x22,
            reserved1: 0x2324,
            buttons_down: 0x2526_2728,
            wheel: -0x1234,
            reserved2: 0x2930,
            x: f64::from_bits(0x7ff8_1234_5678_9abc),
            y: -2.5,
            flags: 0x3132_3334,
        };
        let mut bytes = [0xaa; CURSOR_EVENT_LEN + 2];
        assert!(cursor_event(&mut bytes, &cursor));
        assert_eq!(
            &bytes[..CURSOR_EVENT_LEN],
            &[
                4, 3, 2, 1, 8, 7, 6, 5, 12, 11, 10, 9, 16, 15, 14, 13, 20, 19, 18, 17, 0x21, 0x22,
                0x24, 0x23, 0x28, 0x27, 0x26, 0x25, 0xcc, 0xed, 0x30, 0x29, 0xbc, 0x9a, 0x78, 0x56,
                0x34, 0x12, 0xf8, 0x7f, 0, 0, 0, 0, 0, 0, 4, 0xc0, 0x34, 0x33, 0x32, 0x31, 0, 0, 0,
                0,
            ]
        );
        assert_eq!(&bytes[CURSOR_EVENT_LEN..], &[0xaa, 0xaa]);

        let keyboard = TrueosKeyboardOutputEvent {
            t_ms: 0x0102_0304,
            seq: 0x1112_1314,
            device_seq: 0x2122_2324,
            controller_id: 0x3132_3334,
            slot_id: 0x4142_4344,
            ep_target: 0x5152_5354,
            modifiers: 0x61,
            kind: 0x62,
            utf8_len: 0x63,
            reserved0: 0x64,
            key_code: 0x7172,
            reserved1: 0x8182,
            codepoint: 0x9192_9394,
            utf8: [0xa1, 0xb2, 0xc3, 0xd4],
            flags: 0xe1e2_e3e4,
        };
        let mut bytes = [0xaa; KEYBOARD_OUTPUT_LEN + 2];
        assert!(keyboard_output(&mut bytes, &keyboard));
        assert_eq!(
            &bytes[..KEYBOARD_OUTPUT_LEN],
            &[
                4, 3, 2, 1, 0x14, 0x13, 0x12, 0x11, 0x24, 0x23, 0x22, 0x21, 0x34, 0x33, 0x32, 0x31,
                0x44, 0x43, 0x42, 0x41, 0x54, 0x53, 0x52, 0x51, 0x61, 0x62, 0x63, 0x64, 0x72, 0x71,
                0x82, 0x81, 0x94, 0x93, 0x92, 0x91, 0xa1, 0xb2, 0xc3, 0xd4, 0xe4, 0xe3, 0xe2, 0xe1,
            ]
        );
        assert_eq!(&bytes[KEYBOARD_OUTPUT_LEN..], &[0xaa, 0xaa]);

        let midi = MidiInputEventV1 {
            seq: 0x0102_0304_0506_0708,
            controller_id: 0x1112_1314,
            slot_id: 0x2122_2324,
            channel: 0x31,
            note: 0x32,
            velocity: 0x33,
            gate: 0x34,
        };
        let mut bytes = [0xaa; MIDI_INPUT_V1_LEN + 2];
        assert!(midi_input_v1(&mut bytes, &midi));
        assert_eq!(
            &bytes[..MIDI_INPUT_V1_LEN],
            &[
                8, 7, 6, 5, 4, 3, 2, 1, 0x14, 0x13, 0x12, 0x11, 0x24, 0x23, 0x22, 0x21, 0x31, 0x32,
                0x33, 0x34, 0, 0, 0, 0,
            ]
        );
        assert_eq!(&bytes[MIDI_INPUT_V1_LEN..], &[0xaa, 0xaa]);

        assert_short_buffer_preserved(CURSOR_EVENT_LEN, |out| cursor_event(out, &cursor));
        assert_short_buffer_preserved(KEYBOARD_OUTPUT_LEN, |out| keyboard_output(out, &keyboard));
        assert_short_buffer_preserved(MIDI_INPUT_V1_LEN, |out| midi_input_v1(out, &midi));
    }

    #[test]
    fn control_commands_preserve_reserved_fields_and_short_output_is_rejected() {
        let mouse = MouseMotionCommand {
            opcode: 1,
            path: 2,
            easing: 3,
            flags: 4,
            duration_ms: 0x0506_0708,
            x: -0x1112_1314,
            y: 0x1516_1718,
            control1_x: -0x2122_2324,
            control1_y: 0x2526_2728,
            control2_x: -0x3132_3334,
            control2_y: 0x3536_3738,
            buttons_set: 0x4142_4344,
            buttons_clear: 0x5152_5354,
            wheel: -0x1234,
            reserved: 0x6162,
        };
        let mut bytes = [0xaa; MOUSE_COMMAND_LEN + 2];
        assert!(mouse_command(&mut bytes, &mouse));
        assert_eq!(
            &bytes[..MOUSE_COMMAND_LEN],
            &[
                1, 2, 3, 4, 8, 7, 6, 5, 0xec, 0xec, 0xed, 0xee, 0x18, 0x17, 0x16, 0x15, 0xdc, 0xdc,
                0xdd, 0xde, 0x28, 0x27, 0x26, 0x25, 0xcc, 0xcc, 0xcd, 0xce, 0x38, 0x37, 0x36, 0x35,
                0x44, 0x43, 0x42, 0x41, 0x54, 0x53, 0x52, 0x51, 0xcc, 0xed, 0x62, 0x61,
            ]
        );
        assert_eq!(&bytes[MOUSE_COMMAND_LEN..], &[0xaa, 0xaa]);
        let keyboard = KeyboardControlCommand {
            opcode: 1,
            flags: 2,
            modifiers: 3,
            reserved0: 0x56,
            duration_ms: 0x0708_090a,
            codepoint: 0x1112_1314,
            key_code: 0x2122,
            reserved1: 0x3132,
        };
        let mut bytes = [0xaa; KEYBOARD_COMMAND_LEN + 2];
        assert!(keyboard_command(&mut bytes, &keyboard));
        assert_eq!(
            &bytes[..KEYBOARD_COMMAND_LEN],
            &[
                1, 2, 3, 0x56, 0x0a, 0x09, 0x08, 0x07, 0x14, 0x13, 0x12, 0x11, 0x22, 0x21, 0x32,
                0x31,
            ]
        );
        assert_eq!(&bytes[KEYBOARD_COMMAND_LEN..], &[0xaa, 0xaa]);

        assert_short_buffer_preserved(MOUSE_COMMAND_LEN, |out| mouse_command(out, &mouse));
        assert_short_buffer_preserved(KEYBOARD_COMMAND_LEN, |out| keyboard_command(out, &keyboard));
    }

    fn assert_short_buffer_preserved(size: usize, encode: impl FnOnce(&mut [u8]) -> bool) {
        let mut short = vec![0xaa; size - 1];
        assert!(!encode(&mut short));
        assert!(short.iter().all(|byte| *byte == 0xaa));
    }
}
