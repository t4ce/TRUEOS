//! UTF-8 terminal input and an ANSI sink for Shell3's three control rows.
use super::{MetaFmtStr, RgbaColor, Shell3, SpecialRows};
use alloc::{format, string::String, vec::Vec};

const LINE_LIMIT: usize = 1024;
// Bounded queue large enough for the maximum accepted SSH frame (512 × 256).
pub(super) const OUTPUT_LIMIT: usize = 1024 * 1024;

pub(super) struct Terminal {
    shell: Shell3,
    line: String,
    line_overflow: bool,
    history: Vec<String>,
    history_cursor: Option<usize>,
    history_draft: String,
    utf8: [u8; 4],
    utf8_len: usize,
    escape: u8,
    after_cr: bool,
    prompt_name: String,
    controls: Option<Vec<super::update::RenderedLine>>,
    cursor_column: Option<usize>,
    terminal_active: bool,
    pub output: Vec<u8>,
    pub closing: bool,
    pub overflow: bool,
}

impl Terminal {
    pub fn new(shell: Shell3) -> Self { Self::start(shell, false) }

    pub fn new_ssh(shell: Shell3) -> Self { Self::start(shell, true) }

    fn start(mut shell: Shell3, controls: bool) -> Self {
        shell.set_mode(3);
        let mut terminal = Self {
            shell,
            line: String::new(),
            line_overflow: false,
            history: Vec::new(),
            history_cursor: None,
            history_draft: String::new(),
            utf8: [0; 4],
            utf8_len: 0,
            escape: 0,
            after_cr: false,
            prompt_name: String::new(),
            controls: controls.then(Vec::new),
            cursor_column: None,
            terminal_active: false,
            output: Vec::new(),
            closing: false,
            overflow: false,
        };
        // Save the local wheel mode and stop alternate-screen wheel events
        // from becoming arrow keys in nc's locally echoed input buffer.
        terminal.write(b"\x1b[?1007s\x1b[?1007l\x1b[?1049h\x1b[0m\x1b[2J\x1b[H");
        if controls {
            // Leave mouse handling to the client terminal (selection and menus).
            terminal.write(b"\x1b[1 q\x1b[?25h");
            terminal.set_matrix_region();
            terminal.refresh_controls();
            return terminal;
        }
        let title = terminal.shell.row_for_render(SpecialRows::TitleRow);
        for run in &title.left {
            terminal.write_meta(run);
        }
        terminal.write(b"\r\n");
        terminal.prompt();
        terminal
    }

    fn write(&mut self, bytes: &[u8]) {
        if self.output.len() + bytes.len() > OUTPUT_LIMIT {
            self.overflow = true;
            self.closing = true;
        } else {
            self.output.extend_from_slice(bytes);
        }
    }

    fn write_meta(&mut self, run: &MetaFmtStr) {
        if run.color.is_none() && !run.underline && !run.blink {
            self.write(run.text.as_bytes());
            return;
        }
        self.write(b"\x1b[0m");
        if let Some(color) = run.color {
            let [r, g, b, _] = color.rgba();
            self.write(format!("\x1b[38;2;{r};{g};{b}m").as_bytes());
            if let Some([r, g, b, _]) = color.background() {
                self.write(format!("\x1b[48;2;{r};{g};{b}m").as_bytes());
            }
        }
        if run.underline || run.color.is_some_and(RgbaColor::underline) {
            self.write(b"\x1b[4m");
        }
        if run.blink || run.color.is_some_and(RgbaColor::blink) { self.write(b"\x1b[5m"); }
        self.write(run.text.as_bytes());
        self.write(b"\x1b[0m");
    }

    // One-based inclusive terminal rows; all rows below the controls belong
    // to the selected Matrix buffer. Cursor addressing still uses screen rows.
    fn set_matrix_region(&mut self) {
        let (_, rows) = self.shell.get_size();
        if self.terminal_active { self.write(b"\x1b[r"); }
        else { self.write(format!("\x1b[4;{rows}r").as_bytes()); }
    }

    fn notice(&mut self, text: &str) {
        if self.controls.is_some() {
            self.shell.record_terminal_notice(text);
        } else {
            self.write(b"\r\n");
            self.write(text.as_bytes());
            self.write(b"\r\n");
        }
    }

    fn refresh_controls(&mut self) {
        if self.controls.is_none() || self.closing { return; }
        let app = super::tui::snapshot(self.shell.tui_frontend, self.shell.active_matrix_slot_name().as_deref());
        let active = app.is_some();
        if active != self.terminal_active {
            self.terminal_active = active;
            self.write(b"\x1b[0m\x1b[2J\x1b[H");
            self.set_matrix_region();
            self.controls.as_mut().unwrap().clear();
        }
        let mut current = app.unwrap_or_else(|| self.shell.capture_matrix_snapshot().rendered_lines());
        let previous = self.controls.as_ref().unwrap();
        // The native cursor supplies the blinking block; its underlying cell
        // remains a plain blank, carrying no printable cursor glyph.
        let cursor = if active { None } else { current.get(2).and_then(|row| row.iter().position(|(_, color)| color.is_some_and(RgbaColor::blink))) };
        if let Some(column) = cursor { current[2][column].1 = None; }
        let mut previous = previous.clone();
        let mut shifted = false;
        if !active && previous.len() == current.len() && previous.len() > 4
            && previous[3..] != current[3..]
            && previous[3].len() == current[3].len()
        {
            let height = current.len() - 3;
            for count in 1..height {
                let up = previous[3 + count..] == current[3..current.len() - count];
                let down = previous[3..previous.len() - count] == current[3 + count..];
                if up || down {
                    shifted = true;
                    self.write(format!("\x1b[0m\x1b[4;1H\x1b[{count}{}", if up { 'S' } else { 'T' }).as_bytes());
                    let matrix = &mut previous[3..];
                    if up { matrix.rotate_left(count); } else { matrix.rotate_right(count); }
                    let exposed = if up { height - count..height } else { 0..count };
                    for row in exposed { matrix[row] = alloc::vec![(' ', None); current[3].len()]; }
                    break;
                }
            }
        }
        let updates = super::update::diff_rendered_lines(
            if previous.is_empty() { None } else { Some(&previous) }, &current,
        );
        let changed = shifted || !updates.is_empty();
        for update in updates {
            let row = match update.row {
                SpecialRows::TitleRow => 1,
                SpecialRows::StatusRow => 2,
                SpecialRows::PromtRow => 3,
                SpecialRows::MatrixRow(index) => index + 4,
            };
            self.write(format!("\x1b[{row};{}H", update.offset + 1).as_bytes());
            // Group equal styles, so ordinary text needs no per-cell escapes.
            let mut run = MetaFmtStr::new("");
            for (ch, color) in update.text.chars().zip(update.colors) {
                if run.color != color {
                    self.write_meta(&run);
                    run.text.clear();
                    run.color = color;
                }
                run.text.push(if ch.is_control() { ' ' } else { ch });
            }
            self.write_meta(&run);
            if update.remove > update.text.chars().count() {
                self.write(" ".repeat(update.remove - update.text.chars().count()).as_bytes());
            }
        }
        if cursor.is_some() != self.cursor_column.is_some() {
            self.write(if cursor.is_some() { b"\x1b[?25h" } else { b"\x1b[?25l" });
        }
        if let Some(column) = cursor && (changed || self.cursor_column != cursor) {
            self.write(format!("\x1b[3;{}H", column + 1).as_bytes());
        }
        self.cursor_column = cursor;
        self.controls = Some(current);
    }

    fn prompt(&mut self) {
        let mut prompt = String::from("§");
        if let Some(name) = self.shell.active_matrix_slot_name() {
            prompt.push_str(&name);
        }
        // Match the selected Matrix slot's UI4 highlight.
        self.write_meta(&MetaFmtStr::new(prompt).color(RgbaColor::Pink));
        self.write(b" ");
        self.prompt_name = self.shell.active_matrix_slot_name().unwrap_or_default();
        self.write(b"\x1b7");
    }

    fn reset_input(&mut self) {
        if self.controls.is_some() { return; }
        self.write(b"\x1b8\x1b[K");
        if self.prompt_name != self.shell.active_matrix_slot_name().unwrap_or_default() {
            self.write(b"\r\x1b[2K");
            self.prompt();
        }
    }

    // SSH terminals are created only after authentication. Keep recall local
    // to that connection; plain TCP has no authenticated history contract.
    fn remember(&mut self, line: &str) {
        let command = line.trim();
        let first = command.split_whitespace().next().unwrap_or("");
        if self.controls.is_none() || command.is_empty()
            || first.trim_start_matches('§').eq_ignore_ascii_case("cry")
            || first.bytes().all(|b| b.is_ascii_digit()) { return; }
        if self.history.last().is_some_and(|previous| previous == line) { return; }
        if self.history.len() == 64 { self.history.remove(0); }
        self.history.push(String::from(line));
    }

    fn recall(&mut self, up: bool) {
        if self.controls.is_none() || self.history.is_empty() { return; }
        if up {
            if self.history_cursor.is_none() { self.history_draft = self.line.clone(); }
            let index = self.history_cursor.map_or(self.history.len() - 1, |i| i.saturating_sub(1));
            self.history_cursor = Some(index);
            self.line = self.history[index].clone();
        } else if let Some(index) = self.history_cursor {
            if index + 1 < self.history.len() {
                self.history_cursor = Some(index + 1);
                self.line = self.history[index + 1].clone();
            } else {
                self.history_cursor = None;
                self.line = core::mem::take(&mut self.history_draft);
            }
        }
        self.line_overflow = false;
        self.utf8_len = 0;
        self.shell.set_prompt(&self.line);
        self.shell.set_cursor(self.line.chars().count());
    }

    fn end_recall(&mut self) {
        self.history_cursor = None;
        self.history_draft.clear();
    }

    // SSH is a stream of keyboard events, just like UI4. Only plain nc
    // retains the deferred line replay contract.
    fn keyboard(&mut self, key: Option<u16>, ch: char) {
        use crate::r::keyboard::*;
        let mut completed = String::from(self.shell.prompt());
        if key.is_none() { completed.push(ch); }
        let mut utf8 = [0; 4];
        let utf8_len = if key.is_none() { ch.encode_utf8(&mut utf8).len() as u8 } else { 0 };
        let (_, latched) = self.shell.handle_keyboard_with_latch(&TrueosKeyboardOutputEvent {
            kind: if key.is_some() { KEYBOARD_OUTPUT_KIND_KEY } else { KEYBOARD_OUTPUT_KIND_TEXT },
            key_code: key.unwrap_or(0), codepoint: ch as u32, utf8, utf8_len,
            flags: KEYBOARD_OUTPUT_FLAG_PRESS, ..Default::default()
        });
        if latched { self.remember(&completed); }
        self.line = self.shell.prompt().into();
    }

    fn submit(&mut self) {
        self.reset_input();
        let line = core::mem::take(&mut self.line);
        let recalled = self.history_cursor.is_some();
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
            "help" => self.notice("SSH types directly into Shell3; plain TCP replays on Enter; Backspace erases. Up/Down recalls commands in authenticated SSH sessions.\r\ntab or Tab cycles HV/CMD/ADM; Ctrl-U clears the input line; Ctrl-C cancels.\r\nclear clears the screen (ANSI terminal required).\r\nexit or Ctrl-D on an empty line disconnects.\r\nPlain TCP replay stops at the first name match or impossible prefix; Matrix operators are submitted with Enter."),
            // The remote terminal interprets these bytes; TCP only carries them.
            "clear" => { self.write(b"\x1b[2J\x1b[H"); if let Some(lines) = self.controls.as_mut() { lines.clear(); } else { self.prompt(); } },
            "tab" => {
                if self.controls.is_some() { self.keyboard(Some(crate::r::keyboard::KEYBOARD_KEY_TAB), '\0'); }
                else { self.shell.set_mode(self.shell.get_mode() % 3 + 1); }
            }
            "exit" => {
                if self.controls.is_some() { self.write(b"\x1b[r\x1b[0 q\x1b[?25h"); }
                self.write(b"\x1b[0m\x1b[?1049l\x1b[?1007rBye.\r\n");
                self.closing = true;
            }
            _ if self.controls.is_some() && !recalled => {
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

    fn erase(&mut self) {
        self.end_recall();
        if self.controls.is_some() {
            self.keyboard(Some(crate::r::keyboard::KEYBOARD_KEY_BACKSPACE), '\0');
            return;
        }
        if self.line.pop().is_some() && self.controls.is_none() {
            self.write(b"\x1b8");
            let count = self.line.chars().count();
            if count > 0 { self.write(format!("\x1b[{count}C").as_bytes()); }
            self.write(b"\x1b[K");
        }
    }

    pub(super) fn reconcile_matrix_selection(&mut self) {
        self.shell.reconcile_matrix_selection();
        self.refresh_controls();
    }

    pub(super) fn resize(&mut self, columns: usize, rows: usize) {
        if self.shell.get_size() != (columns, rows) {
            self.shell.set(columns, rows);
            if let Some(lines) = self.controls.as_mut() {
                lines.clear();
                self.write(b"\x1b[0m\x1b[2J\x1b[H");
            }
            if self.controls.is_some() { self.set_matrix_region(); }
            self.refresh_controls();
        }
    }

    pub fn input(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            if self.closing {
                break;
            }
            // A leased crossterm app receives terminal bytes directly, including
            // arrows, Tab, Enter and Ctrl-C/D. Decode UTF-8 through the shared
            // keyboard path so § can still park the lease as it does in UI4.
            if self.controls.is_some() && self.utf8_len == 0 && byte.is_ascii()
                && super::tui::input(self.shell.tui_frontend, self.shell.active_matrix_slot_name().as_deref(), &[byte])
            {
                self.escape = 0;
                self.after_cr = false;
                continue;
            }
            // CRLF can straddle packets; a lone CR or LF also submits once.
            if byte == b'\n' && self.after_cr {
                self.after_cr = false;
                continue;
            }
            self.after_cr = byte == b'\r';
            // Consume CSI/SS3 sequences across arbitrary TCP packets.
            // Unsupported or malformed reports are consumed, never typed.
            if self.escape != 0 {
                if self.escape == 1 {
                    if byte == b'[' || byte == b'O' {
                        self.escape = 2;
                        continue;
                    }
                    self.escape = 0;
                } else if (0x40..=0x7e).contains(&byte) {
                    if self.escape == 2 && matches!(byte, b'A' | b'B') { self.recall(byte == b'A'); }
                    self.escape = 0;
                } else if byte >= 0x20 {
                    self.escape = 3;
                    continue;
                } else {
                    self.escape = 0;
                }
                if byte >= 0x20 { continue; }
            }
            if byte < 0x20 || byte == 0x7f {
                self.utf8_len = 0;
                match byte {
                    b'\r' | b'\n' => self.submit(),
                    8 | 127 => self.erase(),
                    b'\t' => {
                        if self.controls.is_some() { self.keyboard(Some(crate::r::keyboard::KEYBOARD_KEY_TAB), '\0'); }
                        else { self.shell.set_mode(self.shell.get_mode() % 3 + 1); }
                        self.reset_input();
                        let line = self.line.clone();
                        if self.controls.is_none() { self.write(line.as_bytes()); }
                    }
                    3 => {
                        self.end_recall();
                        self.line.clear();
                        if self.controls.is_some() { self.shell.set_prompt(""); }
                        self.line_overflow = false;
                        self.reset_input();
                    }
                    4 if self.line.is_empty() => {
                        if self.controls.is_some() { self.write(b"\x1b[r\x1b[0 q\x1b[?25h"); }
                        self.write(b"\x1b[0m\x1b[?1049l\x1b[?1007r\r\nBye.\r\n");
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
                        self.escape = 1;
                    },
                    _ => {}
                }
                continue;
            }
            // A non-continuation byte starts a fresh character after malformed
            // UTF-8; never lose a valid ASCII byte following a broken prefix.
            if self.utf8_len > 0 && byte & 0xc0 != 0x80 {
                self.utf8_len = 0;
            }
            self.utf8[self.utf8_len] = byte;
            self.utf8_len += 1;
            match core::str::from_utf8(&self.utf8[..self.utf8_len]) {
                Ok(text) => {
                    let ch = text.chars().next().unwrap();
                    if self.line.len() + text.len() > LINE_LIMIT {
                        self.line_overflow = true;
                    }
                    if !ch.is_control() && !self.line_overflow {
                        self.end_recall();
                        if self.controls.is_some() { self.keyboard(None, ch); }
                        else {
                            self.line.push(ch);
                            self.write(b"\x1b8");
                            let offset = self.line.chars().count() - 1;
                            if offset > 0 { self.write(format!("\x1b[{offset}C").as_bytes()); }
                            let encoded = self.utf8;
                            self.write(&encoded[..self.utf8_len]);
                        }
                    } else {
                        self.write(b"\x07");
                    }
                    self.utf8_len = 0;
                }
                Err(error) if error.error_len().is_none() && self.utf8_len < 4 => {}
                Err(_) => {
                    self.utf8_len = 0;
                    self.write(b"\x07");
                }
            }
        }
        if self.controls.is_none() {
            self.shell.set_prompt(&self.line);
            self.shell.set_cursor(self.line.chars().count());
        }
        self.refresh_controls();
    }
}
