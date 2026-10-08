//! UTF-8 terminal input and an ANSI sink for Shell3's three control rows.
use super::{MetaFmtStr, RgbaColor, Shell3, SpecialRows};
use alloc::{format, string::String, vec::Vec};

const LINE_LIMIT: usize = 1024;
pub(super) const OUTPUT_LIMIT: usize = 16 * 1024;

pub(super) struct Terminal {
    shell: Shell3,
    line: String,
    line_overflow: bool,
    utf8: [u8; 4],
    utf8_len: usize,
    escape: u8,
    after_cr: bool,
    prompt_name: String,
    controls: Option<Vec<super::update::RenderedLine>>,
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
            utf8: [0; 4],
            utf8_len: 0,
            escape: 0,
            after_cr: false,
            prompt_name: String::new(),
            controls: controls.then(Vec::new),
            output: Vec::new(),
            closing: false,
            overflow: false,
        };
        // Save the local wheel mode and stop alternate-screen wheel events
        // from becoming arrow keys in nc's locally echoed input buffer.
        terminal.write(b"\x1b[?1007s\x1b[?1007l\x1b[?1049h\x1b[0m\x1b[2J\x1b[H");
        if controls {
            terminal.write(b"\x1b[?25l");
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
        if run.color.is_none() && !run.underline {
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
        self.write(run.text.as_bytes());
        self.write(b"\x1b[0m");
    }

    fn refresh_controls(&mut self) {
        let Some(previous) = self.controls.as_ref() else { return };
        if self.closing { return; }
        let current = self.shell.capture_controls_snapshot().rendered_lines();
        let updates = super::update::diff_rendered_lines(
            if previous.is_empty() { None } else { Some(previous) }, &current,
        );
        for update in updates {
            let row = match update.row {
                SpecialRows::TitleRow => 1,
                SpecialRows::StatusRow => 2,
                SpecialRows::PromtRow => 3,
                _ => continue,
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

    fn submit(&mut self) {
        self.reset_input();
        let line = core::mem::take(&mut self.line);
        let command = line.trim();
        if core::mem::take(&mut self.line_overflow) {
            self.write(b"\r\nInput exceeded 1024 bytes; line discarded.\r\n");
            self.shell.set_prompt("");
            self.reset_input();
            return;
        }
        match command {
            "help" => { if self.controls.is_some() { self.write(b"\x1b[4;1H"); } self.write(b"\r\nUTF-8 line input; Enter replays the line as Shell3 typing; Backspace erases.\r\ntab or Tab cycles HV/CMD/ADM; Ctrl-U clears the input line; Ctrl-C cancels.\r\nclear clears the screen (ANSI terminal required).\r\nexit or Ctrl-D on an empty line disconnects.\r\nThe first name match consumes the line; remaining characters are discarded.\r\nReplay stops at an impossible name prefix; Matrix operators are submitted with Enter.\r\n"); if let Some(lines) = self.controls.as_mut() { lines.clear(); } },
            // The remote terminal interprets these bytes; TCP only carries them.
            "clear" => { self.write(b"\x1b[2J\x1b[H"); if let Some(lines) = self.controls.as_mut() { lines.clear(); } else { self.prompt(); } },
            "tab" => {
                self.shell.set_mode(self.shell.get_mode() % 3 + 1);
            }
            "exit" => {
                if self.controls.is_some() { self.write(b"\x1b[?25h"); }
                self.write(b"\x1b[0m\x1b[?1049l\x1b[?1007rBye.\r\n");
                self.closing = true;
            }
            _ => {
                self.shell.replay_terminal_line(&line);
            }
        }
        // Enter consumes this submission, including any unmatched prefix.
        self.shell.set_prompt("");
        if !self.closing {
            self.reset_input();
        }
    }

    fn erase(&mut self) {
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
                self.write(b"\x1b[0m\x1b[1;1H\x1b[2K\x1b[2;1H\x1b[2K\x1b[3;1H\x1b[2K");
            }
            self.refresh_controls();
        }
    }

    pub fn input(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            if self.closing {
                break;
            }
            // CRLF can straddle packets; a lone CR or LF also submits once.
            if byte == b'\n' && self.after_cr {
                self.after_cr = false;
                continue;
            }
            self.after_cr = byte == b'\r';
            // This line terminal does not implement cursor-key editing. Consume
            // CSI/SS3 sequences so arrow keys do not become command text.
            if self.escape != 0 {
                let was_escape = self.escape == 1;
                self.escape = if was_escape && (byte == b'[' || byte == b'O') {
                    2
                } else if !was_escape
                    && !(0x40..=0x7e).contains(&byte)
                    && byte >= 0x20
                    && self.escape < 32
                {
                    self.escape + 1
                } else {
                    0
                };
                if byte >= 0x20 {
                    continue;
                }
            }
            if byte < 0x20 || byte == 0x7f {
                self.utf8_len = 0;
                match byte {
                    b'\r' | b'\n' => self.submit(),
                    8 | 127 => self.erase(),
                    b'\t' => {
                        self.shell.set_mode(self.shell.get_mode() % 3 + 1);
                        self.reset_input();
                        let line = self.line.clone();
                        if self.controls.is_none() { self.write(line.as_bytes()); }
                    }
                    3 => {
                        self.line.clear();
                        self.line_overflow = false;
                        self.reset_input();
                    }
                    4 if self.line.is_empty() => {
                        if self.controls.is_some() { self.write(b"\x1b[?25h"); }
                        self.write(b"\x1b[0m\x1b[?1049l\x1b[?1007r\r\nBye.\r\n");
                        self.closing = true;
                    }
                    21 => {
                        while !self.line.is_empty() {
                            self.erase();
                        }
                        self.line_overflow = false;
                    }
                    27 => self.escape = 1,
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
                        self.line.push(ch);
                        if self.controls.is_none() {
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
        self.shell.set_prompt(&self.line);
        self.shell.set_cursor(self.line.chars().count());
        self.refresh_controls();
    }
}
