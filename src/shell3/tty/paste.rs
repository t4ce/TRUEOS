//! Bounded bracketed-paste capture; payload bytes never enter keyboard dispatch.
use alloc::{string::String, vec::Vec};
use zeroize::{Zeroize, Zeroizing};

const PASTE_LIMIT: usize = 64 * 1024;
const END: &[u8] = b"\x1b[201~";

pub(super) struct Paste {
    pub(super) slot: Option<String>,
    pub(super) lifetime: Option<u64>,
    pub(super) overflow: bool,
    bytes: Zeroizing<Vec<u8>>,
    end_len: usize,
}

impl Paste {
    pub(super) fn new(slot: Option<String>, lifetime: Option<u64>) -> Self {
        Self {
            slot,
            lifetime,
            overflow: false,
            bytes: Zeroizing::new(Vec::new()),
            end_len: 0,
        }
    }

    fn append(&mut self, bytes: &[u8]) {
        if self.overflow {
            return;
        }
        if self.bytes.len().saturating_add(bytes.len()) > PASTE_LIMIT {
            self.bytes.zeroize();
            self.overflow = true;
        } else {
            self.bytes.extend_from_slice(bytes);
        }
    }

    /// Consume even an oversized paste through its closing marker. A partial
    /// closing marker is held separately, so it can straddle any SSH reads.
    pub(super) fn push(&mut self, byte: u8) -> bool {
        if byte == END[self.end_len] {
            self.end_len += 1;
            return self.end_len == END.len();
        }
        self.append(&END[..self.end_len]);
        self.end_len = 0;
        if byte == END[0] {
            self.end_len = 1;
        } else {
            self.append(&[byte]);
        }
        false
    }

    /// Normalize line endings and tabs; render other controls as text so a
    /// pasted escape sequence cannot become an outbound terminal command.
    pub(super) fn text(&self) -> Zeroizing<String> {
        let decoded = Zeroizing::new(String::from_utf8_lossy(&self.bytes).into_owned());
        let mut text = Zeroizing::new(String::new());
        let mut after_cr = false;
        let mut column = 0;
        for ch in decoded.chars() {
            if ch == '\n' && after_cr {
                after_cr = false;
                continue;
            }
            after_cr = ch == '\r';
            match ch {
                '\r' | '\n' => {
                    text.push('\n');
                    column = 0;
                }
                '\t' => {
                    let spaces = 8 - column % 8;
                    for _ in 0..spaces {
                        text.push(' ');
                    }
                    column += spaces;
                }
                ch if ch.is_control() => {
                    for escaped in ch.escape_default() {
                        text.push(escaped);
                        column += 1;
                    }
                }
                ch => {
                    text.push(ch);
                    column += 1;
                }
            }
        }
        text
    }
}
