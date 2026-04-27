pub const RESET: &str = "\x1b[0m";
pub const SAVE_CURSOR: &str = "\x1b[s";
pub const RESTORE_CURSOR: &str = "\x1b[u";
pub const SHOW_CURSOR: &str = "\x1b[?25h";
pub const CURSOR_COLOR_GRAY: &str = "\x1b]12;#808080\x07";
pub const CURSOR_BLINKING_BLOCK: &str = "\x1b[1 q";
pub const CURSOR_STEADY_BLOCK: &str = "\x1b[2 q";

#[derive(Clone, Copy)]
pub(crate) enum InputDecodeState {
    None,
    Utf8Seq {
        first: u8,
        remaining_continuations: u8,
    },
}

fn utf8_continuation_count(first: u8) -> Option<u8> {
    match first {
        0xC2..=0xDF => Some(1),
        0xE0..=0xEF => Some(2),
        0xF0..=0xF4 => Some(3),
        _ => None,
    }
}

pub(crate) fn decode_input_byte_lossy(state: &mut InputDecodeState, b: u8) -> Option<char> {
    match *state {
        InputDecodeState::None => {
            if let Some(remaining_continuations) = utf8_continuation_count(b) {
                *state = InputDecodeState::Utf8Seq {
                    first: b,
                    remaining_continuations,
                };
                return None;
            }

            if b >= 0x80 {
                return Some('Ü');
            }

            None
        }
        InputDecodeState::Utf8Seq {
            first,
            remaining_continuations,
        } => {
            if (b & 0xC0) != 0x80 {
                *state = InputDecodeState::None;
                return Some('Ü');
            }

            if remaining_continuations > 1 {
                *state = InputDecodeState::Utf8Seq {
                    first,
                    remaining_continuations: remaining_continuations - 1,
                };
                return None;
            }

            *state = InputDecodeState::None;
            if first == 0xC2 && b == 0xA7 {
                Some('§')
            } else {
                Some('Ü')
            }
        }
    }
}

/// Returns the visible terminal column width of `text`.
///
/// This is intended for aligning output that contains ECMA-48/ANSI escape
/// sequences. The width calculation:
/// - ignores `ESC [` (CSI) sequences until the final byte in `@..~`
/// - ignores `ESC ]` (OSC) sequences until BEL (`\x07`) or `ESC \\`
/// - treats control characters as zero-width
/// - counts all other Unicode scalar values as width 1
///
/// Note: This is a pragmatic shell UI helper, not a full terminal emulator.
pub fn visible_width(text: &str) -> usize {
    let bytes = text.as_bytes();
    let mut i = 0usize;
    let mut width = 0usize;

    while i < bytes.len() {
        let b = bytes[i];
        if b == 0x1B {
            // ESC ...
            if i + 1 >= bytes.len() {
                break;
            }
            let next = bytes[i + 1];
            match next {
                b'[' => {
                    // CSI: ESC [ ... <final>
                    i += 2;
                    while i < bytes.len() {
                        let c = bytes[i];
                        // Final byte for CSI is 0x40..=0x7E.
                        i += 1;
                        if (0x40..=0x7E).contains(&c) {
                            break;
                        }
                    }
                    continue;
                }
                b']' => {
                    // OSC: ESC ] ... (BEL | ESC \\)
                    i += 2;
                    while i < bytes.len() {
                        if bytes[i] == 0x07 {
                            i += 1;
                            break;
                        }
                        if bytes[i] == 0x1B && i + 1 < bytes.len() && bytes[i + 1] == b'\\' {
                            i += 2;
                            break;
                        }
                        i += 1;
                    }
                    continue;
                }
                _ => {
                    // Other ESC sequence: skip ESC + next.
                    i += 2;
                    continue;
                }
            }
        }

        // Fast path for ASCII.
        if b < 0x80 {
            i += 1;
            if b >= 0x20 && b != 0x7F {
                width += 1;
            }
            continue;
        }

        // UTF-8 decode the next scalar and count it as width 1.
        // If invalid, consume one byte to avoid infinite loop.
        let s = core::str::from_utf8(&bytes[i..]).ok();
        if let Some(s) = s
            && let Some(ch) = s.chars().next()
        {
            i += ch.len_utf8();
            if !ch.is_control() {
                width += 1;
            }
            continue;
        }
        i += 1;
    }

    width
}
