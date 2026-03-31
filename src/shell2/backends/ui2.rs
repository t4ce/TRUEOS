use alloc::collections::VecDeque;
use alloc::vec::Vec;
use core::fmt::Write;
use core::sync::atomic::{AtomicU32, Ordering};

use spin::{Mutex, Once};

use crate::r::keyboard::{
    KEYBOARD_KEY_ARROW_DOWN, KEYBOARD_KEY_ARROW_LEFT, KEYBOARD_KEY_ARROW_RIGHT,
    KEYBOARD_KEY_ARROW_UP, KEYBOARD_KEY_BACKSPACE, KEYBOARD_KEY_DELETE, KEYBOARD_KEY_END,
    KEYBOARD_KEY_ENTER, KEYBOARD_KEY_ESCAPE, KEYBOARD_KEY_F1, KEYBOARD_KEY_F2, KEYBOARD_KEY_F3,
    KEYBOARD_KEY_F4, KEYBOARD_KEY_F5, KEYBOARD_KEY_HOME, KEYBOARD_KEY_PAGE_DOWN,
    KEYBOARD_KEY_PAGE_UP, KEYBOARD_KEY_TAB, KEYBOARD_OUTPUT_KIND_KEY, KEYBOARD_OUTPUT_KIND_TEXT,
    TrueosKeyboardOutputEvent,
};
use crate::shell2::{ShellBackend2, ShellIo2};

const UI2_SHELL_DEFAULT_FG: (u8, u8, u8) = (0xF1, 0xF4, 0xF8);
const UI2_SHELL_DEFAULT_BG: (u8, u8, u8) = (0x0C, 0x10, 0x16);

pub(crate) struct Ui2ShellBackend;

pub(crate) static UI2_SHELL_BACKEND: Ui2ShellBackend = Ui2ShellBackend;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Ui2ShellCell {
    pub ch: char,
    pub fg: (u8, u8, u8),
    pub bg: (u8, u8, u8),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Ui2ShellScreenSnapshot {
    pub cols: u32,
    pub rows: u32,
    pub cursor_col: u32,
    pub cursor_row: u32,
    pub cursor_visible: bool,
    pub cells: Vec<Ui2ShellCell>,
}

#[derive(Clone, Copy)]
struct TerminalStyle {
    fg: (u8, u8, u8),
    bg: (u8, u8, u8),
    bold: bool,
    dim: bool,
    invert: bool,
}

impl Default for TerminalStyle {
    fn default() -> Self {
        Self {
            fg: UI2_SHELL_DEFAULT_FG,
            bg: UI2_SHELL_DEFAULT_BG,
            bold: false,
            dim: false,
            invert: false,
        }
    }
}

#[derive(Clone, Copy)]
enum EscapeState {
    None,
    Esc,
    Csi,
    Osc,
}

struct TerminalState {
    cols: usize,
    rows: usize,
    cursor_col: usize,
    cursor_row: usize,
    saved_col: usize,
    saved_row: usize,
    scroll_top: usize,
    scroll_bottom: usize,
    cursor_visible: bool,
    style: TerminalStyle,
    cells: Vec<Ui2ShellCell>,
    esc_state: EscapeState,
    csi_buf: Vec<u8>,
    osc_buf: Vec<u8>,
    utf8_buf: [u8; 4],
    utf8_len: usize,
    utf8_expected: usize,
}

impl TerminalState {
    fn new(cols: usize, rows: usize) -> Self {
        let mut out = Self {
            cols: cols.max(1),
            rows: rows.max(1),
            cursor_col: 0,
            cursor_row: 0,
            saved_col: 0,
            saved_row: 0,
            scroll_top: 0,
            scroll_bottom: rows.max(1).saturating_sub(1),
            cursor_visible: true,
            style: TerminalStyle::default(),
            cells: Vec::new(),
            esc_state: EscapeState::None,
            csi_buf: Vec::new(),
            osc_buf: Vec::new(),
            utf8_buf: [0; 4],
            utf8_len: 0,
            utf8_expected: 0,
        };
        out.resize(cols, rows);
        out
    }

    fn resize(&mut self, cols: usize, rows: usize) {
        self.cols = cols.max(1);
        self.rows = rows.max(1);
        self.scroll_top = 0;
        self.scroll_bottom = self.rows.saturating_sub(1);
        self.cursor_col = 0;
        self.cursor_row = 0;
        self.saved_col = 0;
        self.saved_row = 0;
        self.style = TerminalStyle::default();
        self.cells = vec![Self::blank_cell(); self.cols.saturating_mul(self.rows)];
        self.esc_state = EscapeState::None;
        self.csi_buf.clear();
        self.osc_buf.clear();
        self.utf8_len = 0;
        self.utf8_expected = 0;
    }

    fn snapshot(&self) -> Ui2ShellScreenSnapshot {
        Ui2ShellScreenSnapshot {
            cols: self.cols as u32,
            rows: self.rows as u32,
            cursor_col: self.cursor_col as u32,
            cursor_row: self.cursor_row as u32,
            cursor_visible: self.cursor_visible,
            cells: self.cells.clone(),
        }
    }

    fn blank_cell() -> Ui2ShellCell {
        Ui2ShellCell {
            ch: ' ',
            fg: UI2_SHELL_DEFAULT_FG,
            bg: UI2_SHELL_DEFAULT_BG,
        }
    }

    fn cell_index(&self, row: usize, col: usize) -> usize {
        row.saturating_mul(self.cols).saturating_add(col)
    }

    fn set_cell(&mut self, row: usize, col: usize, ch: char) {
        if row >= self.rows || col >= self.cols {
            return;
        }
        let idx = self.cell_index(row, col);
        let (mut fg, mut bg) = (self.style.fg, self.style.bg);
        if self.style.dim {
            fg = (fg.0 / 2, fg.1 / 2, fg.2 / 2);
        }
        if self.style.invert {
            core::mem::swap(&mut fg, &mut bg);
        }
        self.cells[idx] = Ui2ShellCell { ch, fg, bg };
    }

    fn clear_line_range(&mut self, row: usize, start_col: usize, end_col_inclusive: usize) {
        if row >= self.rows {
            return;
        }
        let end = end_col_inclusive.min(self.cols.saturating_sub(1));
        for col in start_col.min(self.cols)..=end {
            let idx = self.cell_index(row, col);
            self.cells[idx] = Self::blank_cell();
        }
    }

    fn clear_all(&mut self) {
        for cell in &mut self.cells {
            *cell = Self::blank_cell();
        }
    }

    fn scroll_up(&mut self, count: usize) {
        if self.scroll_top >= self.rows
            || self.scroll_bottom >= self.rows
            || self.scroll_top > self.scroll_bottom
        {
            return;
        }
        let count = count.max(1).min(
            self.scroll_bottom
                .saturating_sub(self.scroll_top)
                .saturating_add(1),
        );
        for row in self.scroll_top..=self.scroll_bottom.saturating_sub(count) {
            for col in 0..self.cols {
                let dst = self.cell_index(row, col);
                let src = self.cell_index(row + count, col);
                self.cells[dst] = self.cells[src];
            }
        }
        for row in self.scroll_bottom.saturating_sub(count).saturating_add(1)..=self.scroll_bottom {
            self.clear_line_range(row, 0, self.cols.saturating_sub(1));
        }
    }

    fn insert_lines(&mut self, count: usize) {
        if self.cursor_row < self.scroll_top || self.cursor_row > self.scroll_bottom {
            return;
        }
        let count = count.max(1).min(
            self.scroll_bottom
                .saturating_sub(self.cursor_row)
                .saturating_add(1),
        );
        for row in (self.cursor_row..=self.scroll_bottom.saturating_sub(count)).rev() {
            for col in 0..self.cols {
                let dst = self.cell_index(row + count, col);
                let src = self.cell_index(row, col);
                self.cells[dst] = self.cells[src];
            }
        }
        for row in self.cursor_row..self.cursor_row.saturating_add(count) {
            self.clear_line_range(row, 0, self.cols.saturating_sub(1));
        }
    }

    fn line_feed(&mut self) {
        if self.cursor_row == self.scroll_bottom {
            self.scroll_up(1);
        } else {
            self.cursor_row = self
                .cursor_row
                .saturating_add(1)
                .min(self.rows.saturating_sub(1));
        }
    }

    fn put_char(&mut self, ch: char) {
        match ch {
            '\r' => self.cursor_col = 0,
            '\n' => self.line_feed(),
            '\u{0008}' => self.cursor_col = self.cursor_col.saturating_sub(1),
            '\t' => {
                let next_tab = ((self.cursor_col / 8).saturating_add(1)).saturating_mul(8);
                self.cursor_col = next_tab.min(self.cols.saturating_sub(1));
            }
            _ => {
                self.set_cell(self.cursor_row, self.cursor_col, ch);
                self.cursor_col = self.cursor_col.saturating_add(1);
                if self.cursor_col >= self.cols {
                    self.cursor_col = 0;
                    self.line_feed();
                }
            }
        }
    }

    fn feed_byte(&mut self, b: u8) {
        match self.esc_state {
            EscapeState::None => {
                if b == 0x1B {
                    self.esc_state = EscapeState::Esc;
                    return;
                }
                self.feed_text_byte(b);
            }
            EscapeState::Esc => match b {
                b'[' => {
                    self.csi_buf.clear();
                    self.esc_state = EscapeState::Csi;
                }
                b']' => {
                    self.osc_buf.clear();
                    self.esc_state = EscapeState::Osc;
                }
                _ => {
                    self.esc_state = EscapeState::None;
                }
            },
            EscapeState::Csi => {
                if (0x40..=0x7E).contains(&b) {
                    self.exec_csi(b as char);
                    self.csi_buf.clear();
                    self.esc_state = EscapeState::None;
                } else {
                    self.csi_buf.push(b);
                }
            }
            EscapeState::Osc => {
                if b == 0x07 {
                    self.osc_buf.clear();
                    self.esc_state = EscapeState::None;
                } else {
                    self.osc_buf.push(b);
                }
            }
        }
    }

    fn feed_text_byte(&mut self, b: u8) {
        if self.utf8_expected == 0 {
            if b < 0x80 {
                self.put_char(b as char);
                return;
            }
            self.utf8_buf[0] = b;
            self.utf8_len = 1;
            self.utf8_expected = if (b & 0xE0) == 0xC0 {
                2
            } else if (b & 0xF0) == 0xE0 {
                3
            } else if (b & 0xF8) == 0xF0 {
                4
            } else {
                0
            };
            if self.utf8_expected == 0 {
                self.put_char('�');
            }
            return;
        }

        self.utf8_buf[self.utf8_len] = b;
        self.utf8_len += 1;
        if self.utf8_len < self.utf8_expected {
            return;
        }
        let ch = core::str::from_utf8(&self.utf8_buf[..self.utf8_expected])
            .ok()
            .and_then(|text| text.chars().next())
            .unwrap_or('�');
        self.put_char(ch);
        self.utf8_len = 0;
        self.utf8_expected = 0;
    }

    fn parse_params(&self) -> Vec<i32> {
        if self.csi_buf.is_empty() {
            return Vec::new();
        }
        let raw = core::str::from_utf8(self.csi_buf.as_slice()).unwrap_or("");
        let trimmed = raw.trim_start_matches('?').trim_start();
        if trimmed.is_empty() {
            return Vec::new();
        }
        trimmed
            .split(';')
            .map(|part| {
                if part.is_empty() {
                    0
                } else {
                    part.parse::<i32>().unwrap_or(0)
                }
            })
            .collect()
    }

    fn exec_csi(&mut self, final_char: char) {
        let raw = core::str::from_utf8(self.csi_buf.as_slice()).unwrap_or("");
        let params = self.parse_params();
        match final_char {
            'H' | 'f' => {
                let row = params.first().copied().unwrap_or(1).max(1) as usize;
                let col = params.get(1).copied().unwrap_or(1).max(1) as usize;
                self.cursor_row = row.saturating_sub(1).min(self.rows.saturating_sub(1));
                self.cursor_col = col.saturating_sub(1).min(self.cols.saturating_sub(1));
            }
            'J' => match params.first().copied().unwrap_or(0) {
                0 => {
                    self.clear_line_range(
                        self.cursor_row,
                        self.cursor_col,
                        self.cols.saturating_sub(1),
                    );
                    for row in self.cursor_row.saturating_add(1)..self.rows {
                        self.clear_line_range(row, 0, self.cols.saturating_sub(1));
                    }
                }
                1 => {
                    for row in 0..self.cursor_row {
                        self.clear_line_range(row, 0, self.cols.saturating_sub(1));
                    }
                    self.clear_line_range(self.cursor_row, 0, self.cursor_col);
                }
                2 => self.clear_all(),
                _ => {}
            },
            'K' => match params.first().copied().unwrap_or(0) {
                0 => self.clear_line_range(
                    self.cursor_row,
                    self.cursor_col,
                    self.cols.saturating_sub(1),
                ),
                1 => self.clear_line_range(self.cursor_row, 0, self.cursor_col),
                2 => self.clear_line_range(self.cursor_row, 0, self.cols.saturating_sub(1)),
                _ => {}
            },
            'L' => {
                let count = params.first().copied().unwrap_or(1).max(1) as usize;
                self.insert_lines(count);
            }
            'r' => {
                let top = params.first().copied().unwrap_or(1).max(1) as usize;
                let bottom = params.get(1).copied().unwrap_or(self.rows as i32).max(1) as usize;
                if top <= bottom && bottom <= self.rows {
                    self.scroll_top = top.saturating_sub(1);
                    self.scroll_bottom = bottom.saturating_sub(1);
                    self.cursor_row = 0;
                    self.cursor_col = 0;
                } else {
                    self.scroll_top = 0;
                    self.scroll_bottom = self.rows.saturating_sub(1);
                }
            }
            'm' => self.exec_sgr(params.as_slice()),
            's' => {
                self.saved_col = self.cursor_col;
                self.saved_row = self.cursor_row;
            }
            'u' => {
                self.cursor_col = self.saved_col.min(self.cols.saturating_sub(1));
                self.cursor_row = self.saved_row.min(self.rows.saturating_sub(1));
            }
            'q' => {}
            'h' | 'l' => {
                if raw.starts_with("?") && params.first().copied().unwrap_or(0) == 25 {
                    self.cursor_visible = final_char == 'h';
                }
            }
            _ => {}
        }
    }

    fn exec_sgr(&mut self, params: &[i32]) {
        let mut idx = 0usize;
        let codes = if params.is_empty() { &[0][..] } else { params };
        while idx < codes.len() {
            match codes[idx] {
                0 => self.style = TerminalStyle::default(),
                1 => self.style.bold = true,
                2 => self.style.dim = true,
                7 => self.style.invert = true,
                22 => {
                    self.style.bold = false;
                    self.style.dim = false;
                }
                27 => self.style.invert = false,
                30..=37 => self.style.fg = ansi_basic_rgb((codes[idx] - 30) as u8),
                39 => self.style.fg = UI2_SHELL_DEFAULT_FG,
                40..=47 => self.style.bg = ansi_basic_rgb((codes[idx] - 40) as u8),
                49 => self.style.bg = UI2_SHELL_DEFAULT_BG,
                38 | 48 => {
                    let is_fg = codes[idx] == 38;
                    if let Some(mode) = codes.get(idx + 1).copied() {
                        if mode == 5 {
                            if let Some(color) = codes.get(idx + 2).copied() {
                                if is_fg {
                                    self.style.fg = ansi_256_rgb(color as u8);
                                } else {
                                    self.style.bg = ansi_256_rgb(color as u8);
                                }
                            }
                            idx = idx.saturating_add(2);
                        } else if mode == 2 {
                            if idx + 4 < codes.len() {
                                let rgb = (
                                    codes[idx + 2].clamp(0, 255) as u8,
                                    codes[idx + 3].clamp(0, 255) as u8,
                                    codes[idx + 4].clamp(0, 255) as u8,
                                );
                                if is_fg {
                                    self.style.fg = rgb;
                                } else {
                                    self.style.bg = rgb;
                                }
                                idx = idx.saturating_add(4);
                            }
                        }
                    }
                }
                _ => {}
            }
            idx += 1;
        }
    }

    fn feed_bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.feed_byte(b);
        }
    }
}

struct Ui2ShellRuntime {
    window_id: u32,
    dirty_seq: u32,
    input_rx: VecDeque<u8>,
    screen: TerminalState,
}

impl Ui2ShellRuntime {
    fn new() -> Self {
        Self {
            window_id: 0,
            dirty_seq: 0,
            input_rx: VecDeque::new(),
            screen: TerminalState::new(100, 12),
        }
    }

    fn bump_dirty(&mut self) {
        self.dirty_seq = self.dirty_seq.wrapping_add(1).max(1);
    }
}

static UI2_SHELL_RUNTIME: Once<Mutex<Ui2ShellRuntime>> = Once::new();
static UI2_SHELL_RENDERED_SEQ: AtomicU32 = AtomicU32::new(0);

fn runtime() -> &'static Mutex<Ui2ShellRuntime> {
    UI2_SHELL_RUNTIME.call_once(|| Mutex::new(Ui2ShellRuntime::new()))
}

fn ansi_basic_rgb(idx: u8) -> (u8, u8, u8) {
    match idx {
        0 => (0x00, 0x00, 0x00),
        1 => (0x80, 0x00, 0x00),
        2 => (0x00, 0x80, 0x00),
        3 => (0x80, 0x80, 0x00),
        4 => (0x00, 0x00, 0x80),
        5 => (0x80, 0x00, 0x80),
        6 => (0x00, 0x80, 0x80),
        _ => (0xC0, 0xC0, 0xC0),
    }
}

fn ansi_256_rgb(idx: u8) -> (u8, u8, u8) {
    if idx < 16 {
        return match idx {
            0 => (0x00, 0x00, 0x00),
            1 => (0x80, 0x00, 0x00),
            2 => (0x00, 0x80, 0x00),
            3 => (0x80, 0x80, 0x00),
            4 => (0x00, 0x00, 0x80),
            5 => (0x80, 0x00, 0x80),
            6 => (0x00, 0x80, 0x80),
            7 => (0xC0, 0xC0, 0xC0),
            8 => (0x80, 0x80, 0x80),
            9 => (0xFF, 0x00, 0x00),
            10 => (0x00, 0xFF, 0x00),
            11 => (0xFF, 0xFF, 0x00),
            12 => (0x00, 0x00, 0xFF),
            13 => (0xFF, 0x00, 0xFF),
            14 => (0x00, 0xFF, 0xFF),
            _ => (0xFF, 0xFF, 0xFF),
        };
    }
    if idx >= 232 {
        let gray = 8u8.saturating_add((idx - 232).saturating_mul(10));
        return (gray, gray, gray);
    }
    let cube = idx - 16;
    let r = cube / 36;
    let g = (cube % 36) / 6;
    let b = cube % 6;
    let map = |value: u8| -> u8 {
        if value == 0 {
            0
        } else {
            55u8.saturating_add(value.saturating_mul(40))
        }
    };
    (map(r), map(g), map(b))
}

fn push_bytes(bytes: &[u8]) {
    let mut runtime = runtime().lock();
    runtime.screen.feed_bytes(bytes);
    runtime.bump_dirty();
}

pub(crate) fn ui2_shell_attach_window(window_id: u32, cols: usize, rows: usize) {
    let mut runtime = runtime().lock();
    runtime.window_id = window_id;
    runtime.input_rx.clear();
    if runtime.screen.cols != cols.max(1) || runtime.screen.rows != rows.max(1) {
        runtime.screen.resize(cols, rows);
    }
    runtime.bump_dirty();
    UI2_SHELL_RENDERED_SEQ.store(0, Ordering::Release);
}

pub(crate) fn ui2_shell_window_id() -> u32 {
    runtime().lock().window_id
}

pub(crate) fn ui2_shell_dirty_seq() -> u32 {
    runtime().lock().dirty_seq
}

pub(crate) fn ui2_shell_mark_rendered(seq: u32) {
    UI2_SHELL_RENDERED_SEQ.store(seq, Ordering::Release);
}

pub(crate) fn ui2_shell_last_rendered_seq() -> u32 {
    UI2_SHELL_RENDERED_SEQ.load(Ordering::Acquire)
}

pub(crate) fn ui2_shell_snapshot(window_id: u32) -> Option<(u32, Ui2ShellScreenSnapshot)> {
    let runtime = runtime().lock();
    if window_id == 0 || runtime.window_id != window_id {
        return None;
    }
    Some((runtime.dirty_seq, runtime.screen.snapshot()))
}

fn queue_input_bytes(window_id: u32, bytes: &[u8]) -> bool {
    let mut runtime = runtime().lock();
    if window_id == 0 || runtime.window_id != window_id {
        return false;
    }
    runtime.input_rx.extend(bytes.iter().copied());
    true
}

fn queue_key_sequence(window_id: u32, key_code: u16) -> bool {
    match key_code {
        KEYBOARD_KEY_BACKSPACE => queue_input_bytes(window_id, b"\x08"),
        KEYBOARD_KEY_TAB => queue_input_bytes(window_id, b"\t"),
        KEYBOARD_KEY_ENTER => queue_input_bytes(window_id, b"\r"),
        KEYBOARD_KEY_ESCAPE => queue_input_bytes(window_id, b"\x1B"),
        KEYBOARD_KEY_ARROW_UP => queue_input_bytes(window_id, b"\x1B[A"),
        KEYBOARD_KEY_ARROW_DOWN => queue_input_bytes(window_id, b"\x1B[B"),
        KEYBOARD_KEY_ARROW_RIGHT => queue_input_bytes(window_id, b"\x1B[C"),
        KEYBOARD_KEY_ARROW_LEFT => queue_input_bytes(window_id, b"\x1B[D"),
        KEYBOARD_KEY_HOME => queue_input_bytes(window_id, b"\x1B[H"),
        KEYBOARD_KEY_END => queue_input_bytes(window_id, b"\x1B[F"),
        KEYBOARD_KEY_PAGE_UP => queue_input_bytes(window_id, b"\x1B[5~"),
        KEYBOARD_KEY_PAGE_DOWN => queue_input_bytes(window_id, b"\x1B[6~"),
        KEYBOARD_KEY_DELETE => queue_input_bytes(window_id, b"\x7F"),
        KEYBOARD_KEY_F1 => queue_input_bytes(window_id, b"\x1BOP"),
        KEYBOARD_KEY_F2 => queue_input_bytes(window_id, b"\x1BOQ"),
        KEYBOARD_KEY_F3 => queue_input_bytes(window_id, b"\x1BOR"),
        KEYBOARD_KEY_F4 => queue_input_bytes(window_id, b"\x1BOS"),
        KEYBOARD_KEY_F5 => queue_input_bytes(window_id, b"\x1BOT"),
        _ => false,
    }
}

pub(crate) fn queue_ui2_keyboard_event(window_id: u32, event: TrueosKeyboardOutputEvent) -> bool {
    match event.kind {
        KEYBOARD_OUTPUT_KIND_TEXT => {
            let utf8_len = (event.utf8_len as usize).min(event.utf8.len());
            if utf8_len == 0 {
                return false;
            }
            queue_input_bytes(window_id, &event.utf8[..utf8_len])
        }
        KEYBOARD_OUTPUT_KIND_KEY => queue_key_sequence(window_id, event.key_code),
        _ => false,
    }
}

impl ShellIo2 for Ui2ShellBackend {
    fn write_str(&self, s: &str) {
        push_bytes(s.as_bytes());
    }

    fn write_fmt(&self, args: core::fmt::Arguments<'_>) {
        struct Writer;
        impl Write for Writer {
            fn write_str(&mut self, s: &str) -> core::fmt::Result {
                push_bytes(s.as_bytes());
                Ok(())
            }
        }
        let _ = Writer.write_fmt(args);
    }

    fn write_char(&self, ch: char) {
        let mut utf8 = [0u8; 4];
        let text = ch.encode_utf8(&mut utf8);
        push_bytes(text.as_bytes());
    }

    fn write_byte(&self, b: u8) {
        push_bytes(&[b]);
    }
}

impl ShellBackend2 for Ui2ShellBackend {
    fn init(&self) {}

    fn read_byte(&self) -> Option<u8> {
        runtime().lock().input_rx.pop_front()
    }
}
