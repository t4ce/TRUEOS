//! Small UTF-8 line terminal. Socket framing and graphics stay outside it.
use super::{Shell3, SpecialRows, StripSide};
use alloc::{string::String, vec::Vec};

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
    pub output: Vec<u8>,
    pub closing: bool,
    pub overflow: bool,
}

impl Terminal {
    pub fn new(shell: Shell3) -> Self {
        let mut terminal = Self {
            shell,
            line: String::new(),
            line_overflow: false,
            utf8: [0; 4],
            utf8_len: 0,
            escape: 0,
            after_cr: false,
            output: Vec::new(),
            closing: false,
            overflow: false,
        };
        let title = terminal
            .shell
            .get_strip(SpecialRows::TitleRow, StripSide::Left);
        terminal.write(title.as_bytes());
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

    fn prompt(&mut self) {
        self.write("§".as_bytes());
        if let Some(name) = self.shell.active_matrix_slot_name() {
            self.write(name.as_bytes());
        }
        self.write(b" ");
    }

    fn submit(&mut self) {
        self.write(b"\r\n");
        let line = core::mem::take(&mut self.line);
        let command = line.trim();
        if core::mem::take(&mut self.line_overflow) {
            self.write(b"Input exceeded 1024 bytes; line discarded.\r\n");
            self.prompt();
            return;
        }
        match command {
            "help" => self.write(b"UTF-8 line input; Enter replays the line as Shell3 typing; Backspace erases.\r\ntab or Tab cycles HV/CMD/ADM; Ctrl-U clears the input line; Ctrl-C cancels.\r\nclear clears the screen (ANSI terminal required).\r\nexit or Ctrl-D on an empty line disconnects.\r\nThe first name match consumes the line; remaining characters are discarded.\r\nReplay stops at an impossible name prefix; Matrix operators are submitted with Enter.\r\n"),
            // The remote terminal interprets these bytes; TCP only carries them.
            "clear" => self.write(b"\x1b[2J\x1b[H"),
            "tab" => {
                self.shell.set_mode(self.shell.get_mode() % 3 + 1);
            }
            "exit" => {
                self.write(b"Bye.\r\n");
                self.closing = true;
            }
            _ => {
                self.shell.replay_terminal_line(&line);
                self.line = self.shell.prompt().into();
            }
        }
        if !self.closing {
            self.prompt();
            let line = self.line.clone();
            self.write(line.as_bytes());
        }
    }

    fn erase(&mut self) {
        if self.line.pop().is_some() {
            self.write(b"\x08 \x08");
        }
    }

    pub(super) fn reconcile_matrix_selection(&mut self) {
        self.shell.reconcile_matrix_selection();
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
                        self.write(b"\r\n");
                        self.prompt();
                        let line = self.line.clone();
                        self.write(line.as_bytes());
                    }
                    3 => {
                        self.line.clear();
                        self.line_overflow = false;
                        self.write(b"^C\r\n");
                        self.prompt();
                    }
                    4 if self.line.is_empty() => {
                        self.write(b"\r\nBye.\r\n");
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
                        let encoded = self.utf8;
                        self.write(&encoded[..self.utf8_len]);
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
    }
}
