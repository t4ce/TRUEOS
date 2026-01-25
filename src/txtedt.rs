use alloc::vec::Vec;
use embassy_time::{Duration as EmbassyDuration, Timer};
use heapless::String;

use crate::ecma48;
use crate::shell::ShellBackend;

const FILENAME_MAX: usize = 48;
const DIRTY_RGB: (u8, u8, u8) = (255, 55, 255);

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum EscState {
    None,
    Esc,
    Csi,
    Ss3,
}

pub async fn run(
    io: &'static dyn ShellBackend,
    cols: usize,
    rows: usize,
    filename: &str,
    buf: Vec<u8>,
) -> Vec<u8> {
    let mut editor = TextEditor::new(cols, rows, filename, buf);

    io.write_str(ecma48::SHOW_CURSOR);
    editor.redraw(io);

    loop {
        if let Some(b) = io.read_byte() {
            if editor.handle_byte(io, b) {
                break;
            }
        } else {
            Timer::after(EmbassyDuration::from_millis(2)).await;
        }
    }

    io.write_str(ecma48::SHOW_CURSOR);
    let TextEditor { buf, .. } = editor;
    buf
}

struct TextEditor {
    cols: usize,
    rows: usize,
    filename: String<FILENAME_MAX>,

    buf: Vec<u8>,
    cursor: usize, // byte index into buf (0..=len)

    desired_col: usize,
    scroll_line: usize,
    scroll_col: usize,

    dirty: bool,
    esc: EscState,
    csi_param: u16,

    status_msg: String<64>,
}

impl TextEditor {
    fn new(cols: usize, rows: usize, filename: &str, buf: Vec<u8>) -> Self {
        let mut name: String<FILENAME_MAX> = String::new();
        let trimmed = filename.trim();
        let fallback = if trimmed.is_empty() { "untitled.txt" } else { trimmed };
        for ch in fallback.chars() {
            if name.push(ch).is_err() {
                break;
            }
        }

        let mut status_msg: String<64> = String::new();
        let _ = status_msg.push_str("ready");

        let mut this = Self {
            cols,
            rows,
            filename: name,
            buf,
            cursor: 0,
            desired_col: 0,
            scroll_line: 0,
            scroll_col: 0,
            dirty: false,
            esc: EscState::None,
            csi_param: 0,
            status_msg,
        };

        this.cursor = this.buf.len();

        this
    }

    fn handle_byte(&mut self, io: &dyn ShellBackend, b: u8) -> bool {
        // Escape sequence decoder:
        // - arrows: ESC [ A/B/C/D
        // - F1/F2 (SS3): ESC O P / ESC O Q
        // - F1/F2 (CSI): ESC [ 11 ~ / ESC [ 12 ~
        match self.esc {
            EscState::None => {
                if b == 0x1b {
                    self.esc = EscState::Esc;
                    return false;
                }
            }
            EscState::Esc => {
                match b {
                    b'[' => {
                        self.esc = EscState::Csi;
                        self.csi_param = 0;
                    }
                    b'O' => {
                        self.esc = EscState::Ss3;
                    }
                    _ => {
                        self.esc = EscState::None;
                    }
                }
                return false;
            }
            EscState::Csi => {
                // Collect a single numeric parameter (enough for 11~/12~).
                match b {
                    b'0'..=b'9' => {
                        let digit = (b - b'0') as u16;
                        self.csi_param = self.csi_param.saturating_mul(10).saturating_add(digit);
                        return false;
                    }
                    b'A' => self.move_up(),
                    b'B' => self.move_down(),
                    b'C' => self.move_right(),
                    b'D' => self.move_left(),
                    b'~' => {
                        match self.csi_param {
                            11 => {
                                self.save();
                                self.redraw(io);
                                self.esc = EscState::None;
                                return false;
                            }
                            12 => {
                                self.esc = EscState::None;
                                return true;
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }

                self.esc = EscState::None;
                self.ensure_visible();
                self.redraw(io);
                return false;
            }
            EscState::Ss3 => {
                self.esc = EscState::None;
                match b {
                    b'P' => {
                        self.save();
                        self.redraw(io);
                    }
                    b'Q' => return true,
                    _ => {}
                }
                return false;
            }
        }

        match b {
            // Keep Ctrl shortcuts as fallback (useful on serial links without F-keys).
            0x13 => {
                self.save();
                self.redraw(io);
            }
            0x11 => return true,
            // Backspace / DEL
            0x08 | 0x7f => {
                self.backspace();
                self.ensure_visible();
                self.redraw(io);
            }
            // Enter
            b'\r' | b'\n' => {
                self.insert_byte(b'\n');
                self.ensure_visible();
                self.redraw(io);
            }
            // Printable ASCII
            0x20..=0x7e => {
                self.insert_byte(b);
                self.ensure_visible();
                self.redraw(io);
            }
            _ => {}
        }

        false
    }

    fn insert_byte(&mut self, b: u8) {
        let len = self.buf.len();
        if self.cursor > len {
            self.cursor = len;
        }

        // Shift right by 1.
        self.buf.push(0);
        for i in (self.cursor..len).rev() {
            self.buf[i + 1] = self.buf[i];
        }
        self.buf[self.cursor] = b;
        self.cursor += 1;
        self.dirty = true;
        self.desired_col = self.current_line_col().1;
    }

    fn backspace(&mut self) {
        if self.cursor == 0 || self.buf.is_empty() {
            return;
        }
        let len = self.buf.len();
        if self.cursor > len {
            self.cursor = len;
        }
        // Remove byte at cursor-1, shift left.
        let remove_at = self.cursor - 1;
        for i in remove_at..(len - 1) {
            self.buf[i] = self.buf[i + 1];
        }
        let _ = self.buf.pop();
        self.cursor -= 1;
        self.dirty = true;
        self.desired_col = self.current_line_col().1;
    }

    fn move_left(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
        self.desired_col = self.current_line_col().1;
    }

    fn move_right(&mut self) {
        if self.cursor < self.buf.len() {
            self.cursor += 1;
        }
        self.desired_col = self.current_line_col().1;
    }

    fn move_up(&mut self) {
        let (line, _col) = self.current_line_col();
        if line == 0 {
            return;
        }
        let target_line = line - 1;
        self.cursor = self.cursor_for_line_col(target_line, self.desired_col);
    }

    fn move_down(&mut self) {
        let (line, _col) = self.current_line_col();
        let total_lines = self.total_lines();
        if total_lines == 0 {
            return;
        }
        if line + 1 >= total_lines {
            return;
        }
        let target_line = line + 1;
        self.cursor = self.cursor_for_line_col(target_line, self.desired_col);
    }

    fn save(&mut self) {
        self.dirty = false;
        self.set_status("saved (RAM demo)");
    }

    fn total_lines(&self) -> usize {
        // At least one line.
        let mut lines = 1usize;
        for &b in self.buf.iter() {
            if b == b'\n' {
                lines += 1;
            }
        }
        lines
    }

    fn current_line_col(&self) -> (usize, usize) {
        let mut line = 0usize;
        let mut col = 0usize;
        let max = self.cursor.min(self.buf.len());
        for &b in self.buf.iter().take(max) {
            if b == b'\n' {
                line += 1;
                col = 0;
            } else {
                col += 1;
            }
        }
        (line, col)
    }

    fn line_start(&self, line: usize) -> usize {
        if line == 0 {
            return 0;
        }
        let mut cur_line = 0usize;
        for (i, &b) in self.buf.iter().enumerate() {
            if b == b'\n' {
                cur_line += 1;
                if cur_line == line {
                    return i + 1;
                }
            }
        }
        self.buf.len()
    }

    fn line_end(&self, start: usize) -> usize {
        let mut i = start;
        while i < self.buf.len() {
            if self.buf[i] == b'\n' {
                break;
            }
            i += 1;
        }
        i
    }

    fn cursor_for_line_col(&self, line: usize, col: usize) -> usize {
        let start = self.line_start(line);
        let end = self.line_end(start);
        (start + col).min(end)
    }

    fn ensure_visible(&mut self) {
        let cols = self.cols.max(1);
        let rows = self.rows.max(1);

        let view_rows = rows.saturating_sub(2).max(1);
        let view_cols = cols.max(1);

        let (line, col) = self.current_line_col();

        if line < self.scroll_line {
            self.scroll_line = line;
        } else if line >= self.scroll_line + view_rows {
            self.scroll_line = line.saturating_sub(view_rows - 1);
        }

        if col < self.scroll_col {
            self.scroll_col = col;
        } else if col >= self.scroll_col + view_cols {
            self.scroll_col = col.saturating_sub(view_cols - 1);
        }
    }

    fn set_status(&mut self, s: &str) {
        self.status_msg.clear();
        for ch in s.chars() {
            if self.status_msg.push(ch).is_err() {
                break;
            }
        }
    }

    fn redraw(&self, io: &dyn ShellBackend) {
        let cols = self.cols.max(1);
        let rows = self.rows.max(1);
        let view_rows = rows.saturating_sub(2).max(1);
        let view_cols = cols.max(1);

        io.write_str(ecma48::CLEAR_SCREEN);
        io.write_str(ecma48::HOME);

        // Top bar
        io.write_fmt(format_args!("{}", ecma48::invert("")));
        io.write_fmt(format_args!("{}", ecma48::pos(1, 1)));
        io.write_str(ecma48::CLEAR_LINE);

        // Header: render as segments so we can selectively color the filename.
        let mut remaining = view_cols;

        io.write_fmt(format_args!("{}", ecma48::pos(1, 1)));
        write_clipped(io, "TXT ", &mut remaining);

        if remaining > 0 {
            if self.dirty {
                let mut clipped: String<FILENAME_MAX> = String::new();
                for ch in self.filename.chars().take(remaining) {
                    let _ = clipped.push(ch);
                }
                let used = clipped.chars().count();
                io.write_fmt(format_args!("{}", ecma48::color(clipped.as_str(), DIRTY_RGB)));
                remaining = remaining.saturating_sub(used);
            } else {
                let mut used = 0usize;
                for ch in self.filename.chars() {
                    if used >= remaining {
                        break;
                    }
                    io.write_char(ch);
                    used += 1;
                }
                remaining = remaining.saturating_sub(used);
            }
        }

        write_clipped(io, " | F1 save | F2 exit | ←↑→↓ | ", &mut remaining);
        write_clipped(io, self.status_msg.as_str(), &mut remaining);

        // Text area
        let mut line_no = 0usize;
        let mut row = 0usize;
        let mut cur = 0usize;
        let mut col = 0usize;

        // Walk buffer and print only visible window.
        while row < view_rows {
            let screen_row = 2 + row;
            io.write_fmt(format_args!("{}", ecma48::pos(screen_row, 1)));
            io.write_str(ecma48::CLEAR_LINE);

            // Advance until we reach scroll_line.
            while line_no < self.scroll_line && cur < self.buf.len() {
                if self.buf[cur] == b'\n' {
                    line_no += 1;
                }
                cur += 1;
            }

            // Now render this visible line starting at cur.
            let mut local_col = 0usize;
            let mut out_col = 0usize;
            let mut i = cur;
            while i < self.buf.len() {
                let b = self.buf[i];
                if b == b'\n' {
                    break;
                }

                if local_col >= self.scroll_col {
                    if out_col >= view_cols {
                        break;
                    }
                    io.write_char(b as char);
                    out_col += 1;
                }
                local_col += 1;
                i += 1;
            }

            // Move cur to next line
            while cur < self.buf.len() {
                if self.buf[cur] == b'\n' {
                    cur += 1;
                    line_no += 1;
                    break;
                }
                cur += 1;
            }

            row += 1;
        }

        // Bottom bar
        io.write_fmt(format_args!("{}", ecma48::pos(rows, 1)));
        io.write_str(ecma48::CLEAR_LINE);

        let (line, col2) = self.current_line_col();
        let mut footer: String<96> = String::new();
        let _ = footer.push_str("Ln ");
        let _ = push_usize(&mut footer, line + 1);
        let _ = footer.push_str("  Col ");
        let _ = push_usize(&mut footer, col2 + 1);
        let _ = footer.push_str("  Bytes ");
        let _ = push_usize(&mut footer, self.buf.len());

        for (i, ch) in footer.chars().take(view_cols).enumerate() {
            io.write_fmt(format_args!("{}", ecma48::pos(rows, i + 1)));
            io.write_char(ch);
        }

        // Place cursor (1-based). Cursor row is within visible text area.
        let cursor_line_col = self.current_line_col();
        let c_line = cursor_line_col.0;
        let c_col = cursor_line_col.1;

        let vis_line = c_line.saturating_sub(self.scroll_line);
        let vis_col = c_col.saturating_sub(self.scroll_col);

        let cursor_row = 2 + vis_line.min(view_rows.saturating_sub(1));
        let cursor_col = 1 + vis_col.min(view_cols.saturating_sub(1));

        io.write_fmt(format_args!("{}", ecma48::pos(cursor_row, cursor_col)));
    }
}

fn write_clipped(io: &dyn ShellBackend, text: &str, remaining: &mut usize) {
    if *remaining == 0 {
        return;
    }
    let mut used = 0usize;
    for ch in text.chars() {
        if used >= *remaining {
            break;
        }
        io.write_char(ch);
        used += 1;
    }
    *remaining = remaining.saturating_sub(used);
}

fn push_usize<const N: usize>(s: &mut String<N>, mut v: usize) -> core::fmt::Result {
    // minimal, no alloc
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    if v == 0 {
        s.push('0').map_err(|_| core::fmt::Error)?;
        return Ok(());
    }
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    for &b in &buf[i..] {
        s.push(b as char).map_err(|_| core::fmt::Error)?;
    }
    Ok(())
}
