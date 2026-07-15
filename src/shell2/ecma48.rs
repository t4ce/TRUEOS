use unicode_width::UnicodeWidthStr;

pub const RESET: &str = "\x1b[0m";
pub const SAVE_CURSOR: &str = "\x1b[s";
pub const RESTORE_CURSOR: &str = "\x1b[u";
pub const SHOW_CURSOR: &str = "\x1b[?25h";
pub const CURSOR_COLOR_GRAY: &str = "\x1b]12;#808080\x07";
pub const CURSOR_BLINKING_BLOCK: &str = "\x1b[1 q";

/// Returns the visible terminal column width of `text`.
///
/// This is intended for aligning output that contains ECMA-48/ANSI escape
/// sequences. The width calculation:
/// - ignores `ESC [` (CSI) sequences until the final byte in `@..~`
/// - ignores `ESC ]` (OSC) sequences until BEL (`\x07`) or `ESC \\`
/// - uses Unicode terminal-cell widths for visible text
///
/// Note: This is a pragmatic shell UI helper, not a full terminal emulator.
pub fn visible_width(text: &str) -> usize {
    let bytes = text.as_bytes();
    let mut i = 0usize;
    let mut width = 0usize;

    while i < bytes.len() {
        if bytes[i] == 0x1B {
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
                    // Other ESC sequence: skip ESC + the following scalar.
                    i += 1;
                    let scalar_len = text[i..].chars().next().map(char::len_utf8).unwrap_or(0);
                    i += scalar_len;
                    continue;
                }
            }
        }

        let start = i;
        while i < bytes.len() && bytes[i] != 0x1B {
            i += 1;
        }
        width = width.saturating_add(UnicodeWidthStr::width(&text[start..i]));
    }

    width
}

#[cfg(test)]
mod tests {
    use super::visible_width;

    #[test]
    fn measures_international_terminal_cells() {
        assert_eq!(visible_width("§"), 1);
        assert_eq!(visible_width("中国"), 4);
        assert_eq!(visible_width("e\u{301}"), 1);
        assert_eq!(visible_width("👩‍💻"), 2);
    }

    #[test]
    fn excludes_terminal_control_sequences() {
        assert_eq!(visible_width("\x1b[31m中国\x1b[0m"), 4);
        assert_eq!(visible_width("\x1b]12;#808080\x07§"), 1);
    }
}
