//! Scan-code set 2 to HID keyboard state, independent of the i8042 transport.
//!
//! Prefixes survive separate reads. The full bitmap retains keys beyond the
//! six-key compatibility report; releasing a visible key promotes a held key.
//! Pause has no break sequence, so the transport publishes its make and calls
//! `finish_momentary` before sleeping to publish the corresponding release.

const PAUSE_SEQUENCE: [u8; 8] = [0xE1, 0x14, 0x77, 0xE1, 0xF0, 0x14, 0xF0, 0x77];
const HID_PAUSE: u8 = 0x48;

pub(crate) struct Decoder {
    down: [u32; 8],
    keys: [u8; 6],
    extended: bool,
    release: bool,
    pause_index: usize,
}

impl Decoder {
    pub(crate) const fn new() -> Self {
        Self {
            down: [0; 8],
            keys: [0; 6],
            extended: false,
            release: false,
            pause_index: 0,
        }
    }

    /// Consume one scan byte. True means the held-key state changed.
    /// Duplicate make codes (hardware typematic) do not create new presses;
    /// only transitions are published, matching shared HID state handling.
    pub(crate) fn feed(&mut self, byte: u8) -> bool {
        if self.pause_index != 0 {
            if byte == PAUSE_SEQUENCE[self.pause_index] {
                self.pause_index += 1;
                if self.pause_index == PAUSE_SEQUENCE.len() {
                    self.pause_index = 0;
                    return self.set_key(HID_PAUSE, true);
                }
                return false;
            }
            // A truncated/corrupt Pause must not consume the next real key.
            self.reset_prefixes();
        }

        match byte {
            0xE1 => {
                self.reset_prefixes();
                self.pause_index = 1;
                false
            }
            0xE0 => {
                self.extended = true;
                false
            }
            0xF0 => {
                self.release = true;
                false
            }
            // BAT success or a keyboard buffer/error response invalidates the
            // old state. Release it rather than leaving modifiers stuck.
            0xAA | 0x00 | 0xFF | 0xFC | 0xFD => self.clear(),
            // Command replies are not scan codes. Normally consumed by the
            // transport while it initializes the keyboard.
            0xFA | 0xFE | 0xEE => {
                self.reset_prefixes();
                false
            }
            _ => {
                let usage = usage(byte, self.extended);
                let pressed = !self.release;
                self.reset_prefixes();
                usage != 0 && self.set_key(usage, pressed)
            }
        }
    }

    pub(crate) fn modifiers(&self) -> u8 {
        self.down[7] as u8 // HID usages E0..E7 are modifier bits 0..7.
    }

    pub(crate) fn keys(&self) -> [u8; 6] {
        self.keys
    }

    #[cfg(test)]
    pub(crate) fn key_down_bits(&self) -> [u32; 8] {
        self.down
    }

    /// Publish the Pause release after its make, even if no later byte arrives.
    pub(crate) fn finish_momentary(&mut self) -> bool {
        self.set_key(HID_PAUSE, false)
    }

    /// Discard incomplete prefixes and release all held keys after transport
    /// loss, parity errors or keyboard restart. True means a report is needed.
    pub(crate) fn clear(&mut self) -> bool {
        let changed = self.down.iter().any(|&word| word != 0);
        self.down = [0; 8];
        self.keys = [0; 6];
        self.reset_prefixes();
        changed
    }

    fn reset_prefixes(&mut self) {
        self.extended = false;
        self.release = false;
        self.pause_index = 0;
    }

    fn is_down(&self, usage: u8) -> bool {
        self.down[usize::from(usage / 32)] & (1 << (usage % 32)) != 0
    }

    fn set_key(&mut self, usage: u8, pressed: bool) -> bool {
        if self.is_down(usage) == pressed {
            return false;
        }
        let word = &mut self.down[usize::from(usage / 32)];
        let bit = 1 << (usage % 32);
        if pressed {
            *word |= bit;
        } else {
            *word &= !bit;
        }
        if (0xE0..=0xE7).contains(&usage) {
            return true;
        }
        if pressed {
            if let Some(empty) = self.keys.iter_mut().find(|key| **key == 0) {
                *empty = usage;
            }
        } else if let Some(index) = self.keys.iter().position(|&key| key == usage) {
            self.keys[index] = 0;
            // Preserve visible held keys; only fill the released slot.
            if let Some(held) =
                (4..0xE0).find(|&key| self.is_down(key) && !self.keys.contains(&key))
            {
                self.keys[index] = held;
            }
        }
        true
    }
}

/// Physical key positions, using keyboard/keypad HID usages (page 0x07).
/// Layout and text conversion remain the shared keyboard layer's job.
fn usage(scan: u8, extended: bool) -> u8 {
    if extended {
        return match scan {
            0x11 => 0xE6, // right Alt
            0x14 => 0xE4, // right Ctrl
            0x1F => 0xE3, // left GUI
            0x27 => 0xE7, // right GUI
            0x2F => 0x65, // application
            0x4A => 0x54, // keypad /
            0x5A => 0x58, // keypad Enter
            0x69 => 0x4D, // End
            0x6B => 0x50, // left
            0x6C => 0x4A, // Home
            0x70 => 0x49, // Insert
            0x71 => 0x4C, // Delete
            0x72 => 0x51, // down
            0x74 => 0x4F, // right
            0x75 => 0x52, // up
            0x7A => 0x4E, // Page Down
            0x7C => 0x46, // Print Screen; E0 12 fake Shift is ignored.
            0x7D => 0x4B, // Page Up
            _ => 0,
        };
    }
    match scan {
        0x1C => 0x04, // A
        0x32 => 0x05, // B
        0x21 => 0x06, // C
        0x23 => 0x07, // D
        0x24 => 0x08, // E
        0x2B => 0x09, // F
        0x34 => 0x0A, // G
        0x33 => 0x0B, // H
        0x43 => 0x0C, // I
        0x3B => 0x0D, // J
        0x42 => 0x0E, // K
        0x4B => 0x0F, // L
        0x3A => 0x10, // M
        0x31 => 0x11, // N
        0x44 => 0x12, // O
        0x4D => 0x13, // P
        0x15 => 0x14, // Q
        0x2D => 0x15, // R
        0x1B => 0x16, // S
        0x2C => 0x17, // T
        0x3C => 0x18, // U
        0x2A => 0x19, // V
        0x1D => 0x1A, // W
        0x22 => 0x1B, // X
        0x35 => 0x1C, // Y
        0x1A => 0x1D, // Z
        0x16 => 0x1E, // 1
        0x1E => 0x1F, // 2
        0x26 => 0x20, // 3
        0x25 => 0x21, // 4
        0x2E => 0x22, // 5
        0x36 => 0x23, // 6
        0x3D => 0x24, // 7
        0x3E => 0x25, // 8
        0x46 => 0x26, // 9
        0x45 => 0x27, // 0
        0x5A => 0x28, // Enter
        0x76 => 0x29, // Escape
        0x66 => 0x2A, // Backspace
        0x0D => 0x2B, // Tab
        0x29 => 0x2C, // Space
        0x4E => 0x2D, // -
        0x55 => 0x2E, // =
        0x54 => 0x2F, // [
        0x5B => 0x30, // ]
        0x5D => 0x31, // backslash
        0x4C => 0x33, // ;
        0x52 => 0x34, // '
        0x0E => 0x35, // `
        0x41 => 0x36, // ,
        0x49 => 0x37, // .
        0x4A => 0x38, // /
        0x58 => 0x39, // Caps Lock
        0x05 => 0x3A, // F1
        0x06 => 0x3B, // F2
        0x04 => 0x3C, // F3
        0x0C => 0x3D, // F4
        0x03 => 0x3E, // F5
        0x0B => 0x3F, // F6
        0x83 => 0x40, // F7
        0x0A => 0x41, // F8
        0x01 => 0x42, // F9
        0x09 => 0x43, // F10
        0x78 => 0x44, // F11
        0x07 => 0x45, // F12
        0x7E => 0x47, // Scroll Lock
        0x77 => 0x53, // Num Lock
        0x7C => 0x55, // keypad *
        0x7B => 0x56, // keypad -
        0x79 => 0x57, // keypad +
        0x69 => 0x59, // keypad 1
        0x72 => 0x5A, // keypad 2
        0x7A => 0x5B, // keypad 3
        0x6B => 0x5C, // keypad 4
        0x73 => 0x5D, // keypad 5
        0x74 => 0x5E, // keypad 6
        0x6C => 0x5F, // keypad 7
        0x75 => 0x60, // keypad 8
        0x7D => 0x61, // keypad 9
        0x70 => 0x62, // keypad 0
        0x71 => 0x63, // keypad .
        0x61 => 0x64, // ISO extra key
        0x11 => 0xE2, // left Alt
        0x12 => 0xE1, // left Shift
        0x14 => 0xE0, // left Ctrl
        0x59 => 0xE5, // right Shift
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(decoder: &mut Decoder, bytes: &[u8]) -> usize {
        bytes.iter().filter(|&&byte| decoder.feed(byte)).count()
    }

    #[test]
    fn make_break_and_typematic_are_transitions_not_repeated_presses() {
        let mut decoder = Decoder::new();
        assert!(decoder.feed(0x1C));
        assert_eq!(decoder.keys(), [4, 0, 0, 0, 0, 0]);
        assert!(!decoder.feed(0x1C));
        assert!(!decoder.feed(0xF0));
        assert!(decoder.feed(0x1C));
        assert_eq!(decoder.keys(), [0; 6]);
        assert_eq!(decoder.key_down_bits(), [0; 8]);
        assert_eq!(feed(&mut decoder, &[0xF0, 0x1C]), 0);
    }

    #[test]
    fn left_and_right_modifiers_are_independent_of_held_letters() {
        let mut decoder = Decoder::new();
        assert_eq!(feed(&mut decoder, &[0x12, 0x1C, 0xE0, 0x14]), 3);
        assert_eq!(decoder.modifiers(), 0x12);
        assert_eq!(decoder.keys(), [4, 0, 0, 0, 0, 0]);
        assert_eq!(decoder.key_down_bits()[7], (1 << 1) | (1 << 4));
        assert_eq!(feed(&mut decoder, &[0xF0, 0x12, 0xE0, 0xF0, 0x14]), 2);
        assert_eq!(decoder.modifiers(), 0);
        assert_eq!(decoder.keys()[0], 4);
    }

    #[test]
    fn extended_navigation_does_not_alias_keypad() {
        let mut decoder = Decoder::new();
        assert_eq!(feed(&mut decoder, &[0x75, 0xE0, 0x75]), 2);
        assert_eq!(decoder.keys(), [0x60, 0x52, 0, 0, 0, 0]);
        assert_eq!(feed(&mut decoder, &[0xE0, 0xF0, 0x75]), 1);
        assert_eq!(decoder.keys(), [0x60, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn every_left_and_right_modifier_occupies_its_hid_bit() {
        let mut decoder = Decoder::new();
        assert_eq!(
            feed(
                &mut decoder,
                &[
                    0x14, 0x12, 0x11, 0xE0, 0x1F, 0xE0, 0x14, 0x59, 0xE0, 0x11, 0xE0, 0x27,
                ]
            ),
            8
        );
        assert_eq!(decoder.modifiers(), 0xFF);
        assert_eq!(decoder.keys(), [0; 6]);
        assert_eq!(
            feed(
                &mut decoder,
                &[
                    0xF0, 0x14, 0xF0, 0x12, 0xF0, 0x11, 0xE0, 0xF0, 0x1F, 0xE0, 0xF0, 0x14, 0xF0,
                    0x59, 0xE0, 0xF0, 0x11, 0xE0, 0xF0, 0x27,
                ]
            ),
            8
        );
        assert_eq!(decoder.key_down_bits(), [0; 8]);
    }

    #[test]
    fn main_and_keypad_enter_have_separate_make_and_break() {
        let mut decoder = Decoder::new();
        assert_eq!(feed(&mut decoder, &[0x5A, 0xE0, 0x5A]), 2);
        assert_eq!(decoder.keys(), [0x28, 0x58, 0, 0, 0, 0]);
        assert_eq!(feed(&mut decoder, &[0xF0, 0x5A]), 1);
        assert_eq!(decoder.keys(), [0, 0x58, 0, 0, 0, 0]);
        assert_eq!(feed(&mut decoder, &[0xE0, 0xF0, 0x5A]), 1);
        assert_eq!(decoder.key_down_bits(), [0; 8]);
    }

    #[test]
    fn print_screen_fake_shifts_never_change_real_shift_state() {
        let mut decoder = Decoder::new();
        decoder.feed(0x12);
        assert_eq!(feed(&mut decoder, &[0xE0, 0x12, 0xE0, 0x7C]), 1);
        assert_eq!(decoder.modifiers(), 2);
        assert_eq!(decoder.keys()[0], 0x46);
        assert_eq!(feed(&mut decoder, &[0xE0, 0xF0, 0x7C, 0xE0, 0xF0, 0x12]), 1);
        assert_eq!(decoder.modifiers(), 2);
        assert_eq!(decoder.keys(), [0; 6]);
    }

    #[test]
    fn pause_is_one_momentary_key_without_ctrl_or_num_lock() {
        let mut decoder = Decoder::new();
        decoder.feed(0x59);
        assert_eq!(feed(&mut decoder, &PAUSE_SEQUENCE[..7]), 0);
        assert_eq!(decoder.keys(), [0; 6]);
        assert!(decoder.feed(PAUSE_SEQUENCE[7]));
        assert_eq!(decoder.keys()[0], HID_PAUSE);
        assert_eq!(decoder.modifiers(), 0x20);
        assert!(decoder.finish_momentary());
        assert_eq!(decoder.keys(), [0; 6]);
        assert_eq!(decoder.modifiers(), 0x20);
        assert!(!decoder.finish_momentary());
    }

    #[test]
    fn malformed_pause_resynchronizes_at_the_next_key() {
        let mut decoder = Decoder::new();
        assert_eq!(feed(&mut decoder, &[0xE1, 0x14, 0x77, 0x1C]), 1);
        assert_eq!(decoder.keys(), [4, 0, 0, 0, 0, 0]);
        assert_eq!(decoder.modifiers(), 0);
    }

    #[test]
    fn seventh_key_is_retained_without_displacing_visible_held_keys() {
        let mut decoder = Decoder::new();
        // B..G fill the compatibility report; A is retained in the bitmap.
        assert_eq!(
            feed(&mut decoder, &[0x32, 0x21, 0x23, 0x24, 0x2B, 0x34, 0x1C]),
            7
        );
        assert_eq!(decoder.keys(), [5, 6, 7, 8, 9, 10]);
        assert_ne!(decoder.key_down_bits()[0] & (1 << 4), 0);
        assert_eq!(feed(&mut decoder, &[0xF0, 0x23]), 1);
        assert_eq!(decoder.keys(), [5, 6, 4, 8, 9, 10]);
        assert_eq!(feed(&mut decoder, &[0xF0, 0x1C]), 1);
        assert_eq!(decoder.keys(), [5, 6, 0, 8, 9, 10]);
        for scan in [0x32, 0x21, 0x24, 0x2B, 0x34] {
            feed(&mut decoder, &[0xF0, scan]);
        }
        assert_eq!(decoder.key_down_bits(), [0; 8]);
    }

    #[test]
    fn releasing_an_overflow_key_does_not_disturb_the_visible_report() {
        let mut decoder = Decoder::new();
        feed(&mut decoder, &[0x1C, 0x32, 0x21, 0x23, 0x24, 0x2B, 0x34]);
        let visible = decoder.keys();
        assert_eq!(feed(&mut decoder, &[0xF0, 0x34]), 1);
        assert_eq!(decoder.keys(), visible);
        assert_eq!(decoder.key_down_bits()[0] & (1 << 10), 0);
    }

    #[test]
    fn errors_and_bat_clear_state_and_incomplete_prefixes() {
        for response in [0x00, 0xFF, 0xFC, 0xFD, 0xAA] {
            let mut decoder = Decoder::new();
            feed(&mut decoder, &[0x14, 0x1C, 0xE0, 0xF0]);
            assert!(decoder.feed(response));
            assert_eq!(decoder.key_down_bits(), [0; 8]);
            assert!(decoder.feed(0x1C));
            assert_eq!(decoder.modifiers(), 0);
            assert_eq!(decoder.keys()[0], 4);
        }
    }

    #[test]
    fn command_replies_and_unknown_extended_codes_do_not_become_keys() {
        let mut decoder = Decoder::new();
        assert_eq!(feed(&mut decoder, &[0xE0, 0xFA, 0xFE, 0xEE, 0xE0, 0x1C]), 0);
        assert!(decoder.feed(0x1C));
        assert_eq!(decoder.keys(), [4, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn reset_without_held_keys_still_discards_partial_sequences() {
        let mut decoder = Decoder::new();
        feed(&mut decoder, &[0xE0, 0xF0]);
        assert!(!decoder.clear());
        assert!(decoder.feed(0x1C));
        assert_eq!(decoder.keys()[0], 4);
    }
}
