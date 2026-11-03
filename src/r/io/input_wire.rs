//! Explicit little-endian encoders for input records crossing the host/guest ABI.
//! Explicit reserved fields are preserved; implicit C padding is zeroed.

use crate::r::keyboard::TrueosKeyboardOutputEvent;
use crate::usb2::hid::TrueosHidCursorEvent;
use crate::usb3::hid::midi::MidiInputEventV1;
use v::vinput::{KeyboardControlCommand, MouseMotionCommand};

pub(crate) const CURSOR_EVENT_LEN: usize = 56;
pub(crate) const KEYBOARD_OUTPUT_LEN: usize = 44;
pub(crate) const MIDI_INPUT_V1_LEN: usize = 24;
pub(crate) const MOUSE_COMMAND_LEN: usize = 44;
pub(crate) const KEYBOARD_COMMAND_LEN: usize = 16;

macro_rules! field {
    ($ty:ty, $name:ident, $offset:expr, $width:expr, $field_ty:ty) => {
        assert!(core::mem::offset_of!($ty, $name) == $offset);
        const _: fn(&$ty) -> &$field_ty = |value| &value.$name;
        assert!(core::mem::size_of::<$field_ty>() == $width);
    };
}

const _: () = {
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
