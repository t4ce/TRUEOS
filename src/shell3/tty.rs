//! UTF-8 line input and ANSI presentation of Shell3's shared visual snapshot.
use super::{Shell3, update::RenderedLine};
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
    presented: Vec<RenderedLine>,
    presented_cursor: Option<usize>,
    scroll_rows: usize,
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
            presented: Vec::new(),
            presented_cursor: None,
            scroll_rows: 0,
            output: Vec::new(),
            closing: false,
            overflow: false,
        };
        terminal.write("\x1b]0;TrueOS §\x07".as_bytes());
        terminal.write(b"\x1b[2J\x1b[H");
        terminal.present();
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

    /// Consume the same composed cells as UI4, at Shell3's fixed default size.
    fn present(&mut self) {
        if self.closing { return; }
        let snapshot = self.shell.capture_update_snapshot();
        let lines = snapshot.rendered_lines();
        let cursor = self.shell.cursor().min(snapshot.size().0.saturating_sub(1));
        let mut changed = self.presented_cursor != Some(cursor);
        if self.scroll_rows != snapshot.size().1 {
            self.scroll_rows = snapshot.size().1;
            self.write(format!("\x1b[4;{}r", self.scroll_rows).as_bytes());
            changed = true;
        }
        // A prepend moves older Matrix rows down, dropping the bottom row.
        // Let the terminal do that within the body, then paint the new rows.
        if !snapshot.terminal_active() && lines.len() == self.presented.len() && lines.len() > 4 {
            let body = &lines[3..];
            let previous = &self.presented[3..];
            if body != previous {
                if let Some(count) = (1..body.len()).find(|&n| body[n..] == previous[..body.len() - n]) {
                    self.write(format!("\x1b[4;1H\x1b[0m\x1b[{count}T").as_bytes());
                    self.presented[3..].rotate_right(count);
                    for line in &mut self.presented[3..3 + count] {
                        line.fill((' ', None));
                    }
                    changed = true;
                }
            }
        }
        for row in 0..lines.len().max(self.presented.len()) {
            let line = lines.get(row).map(Vec::as_slice).unwrap_or(&[]);
            // nc's local echo may have touched the prompt even when a whole
            // submitted line leaves the model unchanged. Repaint it on input.
            if self.presented.get(row).map(Vec::as_slice) == Some(line)
                && !(row == 2 && self.presented_cursor.is_none()) { continue; }
            changed = true;
            self.write(format!("\x1b[{};1H\x1b[0m\x1b[2K", row + 1).as_bytes());
            let end = line.iter().rposition(|cell| *cell != (' ', None)).map_or(0, |i| i + 1);
            let mut style = None;
            for &(ch, color) in &line[..end] {
                if color != style {
                    self.write(b"\x1b[0m");
                    if let Some(color) = color {
                        let [r, g, b, _] = color.rgba();
                        self.write(format!("\x1b[38;2;{r};{g};{b}m").as_bytes());
                        if let Some([r, g, b, _]) = color.background() {
                            self.write(format!("\x1b[48;2;{r};{g};{b}m").as_bytes());
                        }
                        if color.underline() { self.write(b"\x1b[4m"); }
                    }
                    style = color;
                }
                let mut utf8 = [0; 4];
                self.write(ch.encode_utf8(&mut utf8).as_bytes());
            }
        }
        if changed {
            self.write(format!("\x1b[0m\x1b[3;{}H", cursor + 1).as_bytes());
        }
        self.presented = lines;
        self.presented_cursor = Some(cursor);
    }

    fn submit(&mut self) {
        let line = core::mem::take(&mut self.line);
        let command = line.trim();
        if core::mem::take(&mut self.line_overflow) {
            self.shell.terminal_message("Input exceeded 1024 bytes; line discarded.");
            self.shell.set_prompt("");
            return;
        }
        match command {
            "help" => self.shell.terminal_message("UTF-8 line input; Enter replays the line as Shell3 typing; Backspace erases.\r\ntab or Tab cycles HV/CMD/ADM; Ctrl-U clears the input line; Ctrl-C cancels.\r\nclear clears the screen (ANSI terminal required).\r\nexit or Ctrl-D on an empty line disconnects.\r\nThe first name match consumes the line; remaining characters are discarded.\r\nReplay stops at an impossible name prefix; Matrix operators are submitted with Enter.\r\n"),
            // The remote terminal interprets these bytes; TCP only carries them.
            "clear" => {
                self.write(b"\x1b[2J\x1b[H");
                self.presented.clear();
                self.presented_cursor = None;
            }
            "tab" => {
                self.shell.set_mode(self.shell.get_mode() % 3 + 1);
            }
            "exit" => {
                self.write(b"\x1b[rBye.\r\n");
                self.closing = true;
            }
            _ => {
                self.shell.replay_terminal_line(&line);
            }
        }
        // Enter consumes the entire submission, including unmatched prefixes.
        // Never seed the next network line with the replay's temporary prompt.
        self.shell.set_prompt("");
    }

    fn erase(&mut self) {
        self.line.pop();
    }

    pub(super) fn reconcile_matrix_selection(&mut self) {
        self.shell.reconcile_matrix_selection();
        self.present();
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
                    }
                    3 => {
                        self.line.clear();
                        self.line_overflow = false;
                    }
                    4 if self.line.is_empty() => {
                        self.write(b"\x1b[r\r\nBye.\r\n");
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
        if !bytes.is_empty() { self.presented_cursor = None; }
        self.present();
    }
}
