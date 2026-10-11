//! UTF-8/control decoding, shared keyboard dispatch, and session-local recall.
use super::super::tui;
use super::{LINE_LIMIT, Terminal};
use alloc::{format, string::String, vec::Vec};
use zeroize::{Zeroize, Zeroizing};

/// Partial transport input that can span several network packets.
#[derive(Default)]
pub(super) struct InputDecoder {
    pub(super) utf8: [u8; 4],
    pub(super) utf8_len: usize,
    pub(super) escape: u8,
    pub(super) csi: Vec<u8>,
    pub(super) csi_overflow: bool,
    pub(super) after_cr: bool,
}

impl Terminal {
    pub(super) fn remember(&mut self, line: &str) {
        if self.view.lines.is_some() { self.history.remember(line); }
    }

    pub(super) fn recall(&mut self, up: bool) {
        if self.view.lines.is_none() { return; }
        let Some(line) = self.history.recall(up, &self.line) else { return; };
        self.line.zeroize();
        self.line = line;
        self.line_overflow = false;
        self.decoder.utf8_len = 0;
        self.shell.set_prompt(&self.line);
        self.shell.set_cursor(self.line.chars().count());
    }

    pub(super) fn end_recall(&mut self) { self.history.end_recall(); }

    // SSH and UI4 share immediate recognition; nc alone defers until Enter.
    pub(super) fn keyboard(&mut self, key: Option<u16>, ch: char) {
        use crate::r::keyboard::*;
        let mut completed = Zeroizing::new(String::from(self.shell.prompt()));
        if key.is_none() {
            completed.push(ch);
        }
        let mut utf8 = [0; 4];
        let utf8_len = if key.is_none() {
            ch.encode_utf8(&mut utf8).len() as u8
        } else {
            0
        };
        let (_, latched) = self
            .shell
            .handle_keyboard_with_latch(&TrueosKeyboardOutputEvent {
                kind: if key.is_some() {
                    KEYBOARD_OUTPUT_KIND_KEY
                } else {
                    KEYBOARD_OUTPUT_KIND_TEXT
                },
                key_code: key.unwrap_or(0),
                codepoint: ch as u32,
                utf8,
                utf8_len,
                flags: KEYBOARD_OUTPUT_FLAG_PRESS,
                ..Default::default()
            });
        if latched {
            if key == Some(KEYBOARD_KEY_TAB) {
                let end = completed.char_indices()
                    .map(|(index, ch)| index + ch.len_utf8())
                    .find(|&end| self.shell.parse_name(&completed[..end]));
                if let Some(end) = end { completed.truncate(end); }
                else { completed.zeroize(); }
            }
            self.remember(&completed);
        }
        self.line.zeroize();
        self.line = self.shell.prompt().into();
    }

    pub(super) fn submit(&mut self) {
        self.reset_input();
        let line = Zeroizing::new(core::mem::take(&mut self.line));
        let recalled = self.history.cursor.is_some();
        self.end_recall();
        let command = line.trim();
        if core::mem::take(&mut self.line_overflow) {
            self.notice("Input exceeded 1024 bytes; line discarded.");
            self.shell.set_prompt("");
            self.reset_input();
            return;
        }
        self.remember(&line);
        match command {
            "help" => self.notice("SSH types directly into Shell3; plain TCP replays on Enter; Backspace erases. Up/Down recalls commands in authenticated SSH sessions. PageUp/PageDown scroll the Matrix; Home/End jump to its top/bottom.\r\ntab or Tab cycles HV/CMD/ADM; Ctrl-U clears the input line; Ctrl-C cancels.\r\nclear clears the screen (ANSI terminal required).\r\nexit or Ctrl-D on an empty line disconnects.\r\nPlain TCP replay stops at the first name match or impossible prefix; Matrix operators are submitted with Enter."),
            // The remote terminal interprets these bytes; TCP only carries them.
            "clear" => { self.clear_screen(); if let Some(lines) = self.view.lines.as_mut() { lines.clear(); } else { self.prompt(); } },
            "tab" => {
                if self.view.lines.is_some() { self.keyboard(Some(crate::r::keyboard::KEYBOARD_KEY_TAB), '\0'); }
                else { self.shell.set_mode(self.shell.get_mode() % 3 + 1); }
            }
            "exit" => {
                if self.view.lines.is_some() { self.write(b"\x1b[r\x1b[0 q\x1b[?25h"); }
                self.write(b"\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l\x1b[0m\x1b[?1049l\x1b[?1007rBye.\r\n");
                self.closing = true;
            }
            _ if self.view.lines.is_some() && !recalled => {
                self.keyboard(Some(crate::r::keyboard::KEYBOARD_KEY_ENTER), '\0');
                return;
            }
            _ => { self.shell.replay_terminal_line(&line); }
        }
        // Enter consumes this submission, including any unmatched prefix.
        self.shell.set_prompt("");
        if !self.closing {
            self.reset_input();
        }
    }

    pub(super) fn erase(&mut self) {
        self.end_recall();
        if self.view.lines.is_some() {
            self.keyboard(Some(crate::r::keyboard::KEYBOARD_KEY_BACKSPACE), '\0');
            return;
        }
        if self.line.pop().is_some() && self.view.lines.is_none() {
            self.write(b"\x1b8");
            let count = self.line.chars().count();
            if count > 0 {
                self.write(format!("\x1b[{count}C").as_bytes());
            }
            self.write(b"\x1b[K");
        }
    }

    // Scroll only Shell3's transcript. Leased apps receive their raw keys
    // earlier in input(), and plain TCP retains its line-oriented behavior.
    fn matrix_scroll_key(&mut self, final_byte: u8) {
        if self.view.lines.is_none() || self.decoder.csi_overflow { return; }
        let page = self.shell.get_size().1.saturating_sub(3).max(1).min(i32::MAX as usize) as i32;
        let rows = match (final_byte, self.decoder.csi.as_slice()) {
            (b'~', b"5") => -page,
            (b'~', b"6") => page,
            (b'H', b"") | (b'~', b"1" | b"7") => i32::MIN,
            (b'F', b"") | (b'~', b"4" | b"8") => i32::MAX,
            _ => return,
        };
        self.shell.scroll_matrix(rows);
    }

    // SGR mouse coordinates are terminal cells, unlike UI4's pixel coordinates.
    fn matrix_mouse(&mut self, final_byte: u8) {
        if self.view.lines.is_none() || self.decoder.csi_overflow || !matches!(final_byte, b'M' | b'm') { return; }
        let Some(parameters) = self.decoder.csi.strip_prefix(b"<") else { return; };
        let Ok(parameters) = core::str::from_utf8(parameters) else { return; };
        let mut fields = parameters.split(';');
        let Some(button) = fields.next().and_then(|v| v.parse::<u32>().ok()) else { return; };
        let Some(x) = fields.next().and_then(|v| v.parse::<i32>().ok()).filter(|v| *v > 0) else { return; };
        let Some(y) = fields.next().and_then(|v| v.parse::<i32>().ok()).filter(|v| *v > 0) else { return; };
        if fields.next().is_some() || button & 64 != 0 || button & 3 != 1 { return; }
        let (columns, rows) = self.shell.get_size();
        let inside = x as usize <= columns && (4..=rows).contains(&(y as usize));
        self.shell.drag_matrix((x - 1, y - 1), final_byte == b'M' && button & 32 == 0,
            final_byte == b'M', inside, (1, 1));
    }

    pub fn input(&mut self, mut bytes: &[u8]) {
        // The locked § operator must pass through UTF-8 decoding, including
        // when its C2 A7 bytes arrive in separate SSH packets.
        if self.decoder.utf8_len == 0 && !bytes.contains(&0xc2)
            && tui::remote_active(self.shell.tui_frontend)
            && tui::input(self.shell.tui_frontend, self.shell.active_matrix_slot_name().as_deref(), bytes)
        {
            self.shell.drag_matrix((0, 0), false, false, false, (1, 1));
            self.refresh_controls();
            return;
        }
        while let Some((&byte, rest)) = bytes.split_first() {
            if self.closing {
                break;
            }
            // A leased crossterm app receives terminal bytes directly, including
            // arrows, Tab, Enter and Ctrl-C/D. Submit available ASCII together:
            // waking the app for ESC alone can turn a mouse report into Esc.
            // Decode UTF-8 through the shared
            // keyboard path so § can still park the lease as it does in UI4.
            let ascii_len =
                if (self.view.lines.is_some() || tui::remote_active(self.shell.tui_frontend)) && self.decoder.utf8_len == 0 && byte.is_ascii() {
                    bytes.iter().take_while(|byte| byte.is_ascii()).count()
                } else {
                    0
                };
            if ascii_len > 0
                && tui::input(
                    self.shell.tui_frontend,
                    self.shell.active_matrix_slot_name().as_deref(),
                    &bytes[..ascii_len],
                )
            {
                self.decoder.escape = 0;
                self.decoder.after_cr = false;
                bytes = &bytes[ascii_len..];
                continue;
            }
            bytes = rest;
            // CRLF can straddle packets; a lone CR or LF also submits once.
            if byte == b'\n' && self.decoder.after_cr {
                self.decoder.after_cr = false;
                continue;
            }
            self.decoder.after_cr = byte == b'\r';
            // Consume CSI/SS3 sequences across arbitrary TCP packets.
            // Unsupported or malformed reports are consumed, never typed.
            if self.decoder.escape != 0 {
                if self.decoder.escape == 1 {
                    if byte == b'[' || byte == b'O' {
                        self.decoder.escape = 2;
                        self.decoder.csi.clear();
                        self.decoder.csi_overflow = false;
                        continue;
                    }
                    self.decoder.escape = 0;
                } else if (0x40..=0x7e).contains(&byte) {
                    self.matrix_mouse(byte);
                    self.matrix_scroll_key(byte);
                    let arrow_parameters = matches!(self.decoder.csi.as_slice(), b"" | b"0" | b"1")
                        || self.decoder.csi.strip_prefix(b"1;").is_some_and(|modifier| matches!(modifier, b"2" | b"3" | b"4" | b"5" | b"6" | b"7" | b"8"));
                    if !self.decoder.csi_overflow && arrow_parameters && matches!(byte, b'A' | b'B') {
                        self.recall(byte == b'A');
                    }
                    self.decoder.escape = 0;
                } else if byte >= 0x20 {
                    if self.decoder.csi.len() < 48 { self.decoder.csi.push(byte); }
                    else { self.decoder.csi_overflow = true; }
                    self.decoder.escape = 3;
                    continue;
                } else {
                    self.decoder.escape = 0;
                }
                if byte >= 0x20 {
                    continue;
                }
            }
            if byte < 0x20 || byte == 0x7f {
                self.decoder.utf8_len = 0;
                match byte {
                    b'\r' | b'\n' => self.submit(),
                    8 | 127 => self.erase(),
                    b'\t' => {
                        if self.view.lines.is_some() {
                            self.keyboard(Some(crate::r::keyboard::KEYBOARD_KEY_TAB), '\0');
                        } else {
                            self.shell.set_mode(self.shell.get_mode() % 3 + 1);
                        }
                        self.reset_input();
                        let line = self.line.clone();
                        if self.view.lines.is_none() {
                            self.write(line.as_bytes());
                        }
                    }
                    3 => {
                        self.end_recall();
                        self.line.zeroize();
                        if self.view.lines.is_some() {
                            self.shell.set_prompt("");
                        }
                        self.line_overflow = false;
                        self.reset_input();
                    }
                    4 if self.line.is_empty() => {
                        if self.view.lines.is_some() {
                            self.write(b"\x1b[r\x1b[0 q\x1b[?25h");
                        }
                        self.write(b"\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l\x1b[0m\x1b[?1049l\x1b[?1007r\r\nBye.\r\n");
                        self.closing = true;
                    }
                    21 => {
                        self.end_recall();
                        while !self.line.is_empty() {
                            self.erase();
                        }
                        self.line_overflow = false;
                    }
                    27 => {
                        self.decoder.escape = 1;
                    }
                    _ => {}
                }
                continue;
            }
            // A non-continuation byte starts a fresh character after malformed
            // UTF-8; never lose a valid ASCII byte following a broken prefix.
            if self.decoder.utf8_len > 0 && byte & 0xc0 != 0x80 {
                self.decoder.utf8_len = 0;
            }
            self.decoder.utf8[self.decoder.utf8_len] = byte;
            self.decoder.utf8_len += 1;
            match core::str::from_utf8(&self.decoder.utf8[..self.decoder.utf8_len]) {
                Ok(text) => {
                    let ch = text.chars().next().unwrap();
                    if self.line.len() + text.len() > LINE_LIMIT {
                        self.line_overflow = true;
                    }
                    if !ch.is_control() && !self.line_overflow {
                        self.end_recall();
                        if self.view.lines.is_some() {
                            self.keyboard(None, ch);
                        } else {
                            self.line.push(ch);
                            self.write(b"\x1b8");
                            let offset = self.line.chars().count() - 1;
                            if offset > 0 {
                                self.write(format!("\x1b[{offset}C").as_bytes());
                            }
                            let encoded = self.decoder.utf8;
                            self.write(&encoded[..self.decoder.utf8_len]);
                        }
                    } else {
                        self.write(b"\x07");
                    }
                    self.decoder.utf8_len = 0;
                }
                Err(error) if error.error_len().is_none() && self.decoder.utf8_len < 4 => {}
                Err(_) => {
                    self.decoder.utf8_len = 0;
                    self.write(b"\x07");
                }
            }
        }
        if self.view.lines.is_none() {
            self.shell.set_prompt(&self.line);
            self.shell.set_cursor(self.line.chars().count());
        }
        self.refresh_controls();
    }
}
