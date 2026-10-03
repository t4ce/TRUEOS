#![no_std]

extern crate alloc;

use alloc::{string::String, vec, vec::Vec};

const TAB_WIDTH: usize = 8;
const MAX_CSI_BYTES: usize = 128;
const MAX_OSC_BYTES: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Cursor {
    pub col: usize,
    pub row: usize,
    pub visible: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
}

impl MouseButton {
    const fn protocol_code(self) -> u8 {
        match self {
            Self::Left => 0,
            Self::Middle => 1,
            Self::Right => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ParserState {
    Ground,
    Escape,
    Csi,
    Osc,
    OscEscape,
}

/// A terminal color suitable for direct renderer consumption.
///
/// Indexed values use the conventional 256-color terminal palette. Values
/// `0..=7` are standard ANSI colors and `8..=15` are their bright variants.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TerminalColor {
    #[default]
    Default,
    Indexed(u8),
    Rgb {
        red: u8,
        green: u8,
        blue: u8,
    },
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CellStyle {
    pub foreground: TerminalColor,
    pub background: TerminalColor,
    pub underline: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Cell {
    pub glyph: char,
    pub style: CellStyle,
}

impl Cell {
    const fn blank() -> Self {
        Self {
            glyph: ' ',
            style: CellStyle {
                foreground: TerminalColor::Default,
                background: TerminalColor::Default,
                underline: false,
            },
        }
    }
}

/// A compact, fixed-cell terminal screen for the UI4 shell frontend.
///
/// Every Unicode scalar occupies exactly one cell. Foreground, background,
/// and underline SGR state is captured on each written cell; unsupported
/// styling remains ignored.
pub struct Terminal {
    cols: usize,
    rows: usize,
    cells: Vec<Cell>,
    current_style: CellStyle,
    cursor_col: usize,
    cursor_row: usize,
    saved_col: usize,
    saved_row: usize,
    cursor_visible: bool,
    pending_wrap: bool,
    scroll_top: usize,
    scroll_bottom: usize,
    parser_state: ParserState,
    csi_buf: Vec<u8>,
    csi_overflow: bool,
    osc_buf: Vec<u8>,
    osc_overflow: bool,
    utf8_buf: [u8; 4],
    utf8_len: usize,
    utf8_expected: usize,
    responses: Vec<u8>,
    mouse_normal_tracking: bool,
    mouse_button_tracking: bool,
    mouse_any_tracking: bool,
    mouse_sgr_encoding: bool,
    mouse_urxvt_encoding: bool,
    dirty: bool,
}

impl Terminal {
    pub fn new(cols: usize, rows: usize) -> Self {
        let cols = cols.max(1);
        let rows = rows.max(1);
        Self {
            cols,
            rows,
            cells: vec![Cell::blank(); cols.saturating_mul(rows)],
            current_style: CellStyle::default(),
            cursor_col: 0,
            cursor_row: 0,
            saved_col: 0,
            saved_row: 0,
            cursor_visible: true,
            pending_wrap: false,
            scroll_top: 0,
            scroll_bottom: rows - 1,
            parser_state: ParserState::Ground,
            csi_buf: Vec::new(),
            csi_overflow: false,
            osc_buf: Vec::new(),
            osc_overflow: false,
            utf8_buf: [0; 4],
            utf8_len: 0,
            utf8_expected: 0,
            responses: Vec::new(),
            mouse_normal_tracking: false,
            mouse_button_tracking: false,
            mouse_any_tracking: false,
            mouse_sgr_encoding: false,
            mouse_urxvt_encoding: false,
            dirty: true,
        }
    }

    pub fn dimensions(&self) -> (usize, usize) {
        (self.cols, self.rows)
    }

    /// Return the row-major screen cells. Use `dimensions` to split rows.
    pub fn cells(&self) -> &[Cell] {
        &self.cells
    }

    pub fn cursor(&self) -> Cursor {
        Cursor {
            col: self.cursor_col,
            row: self.cursor_row,
            visible: self.cursor_visible,
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn take_dirty(&mut self) -> bool {
        core::mem::replace(&mut self.dirty, false)
    }

    /// Take terminal-generated replies such as device attributes and cursor
    /// position reports. The frontend sends these back to the current direct
    /// handoff owner through the same channel as keyboard input.
    pub fn take_responses(&mut self) -> Vec<u8> {
        core::mem::take(&mut self.responses)
    }

    pub fn mouse_button(
        &self,
        button: MouseButton,
        pressed: bool,
        col: usize,
        row: usize,
    ) -> Option<Vec<u8>> {
        if !self.mouse_tracking_enabled() {
            return None;
        }
        Some(self.encode_mouse(button.protocol_code(), !pressed, col, row))
    }

    pub fn mouse_motion(
        &self,
        held_button: Option<MouseButton>,
        col: usize,
        row: usize,
    ) -> Option<Vec<u8>> {
        if !self.mouse_any_tracking && !(self.mouse_button_tracking && held_button.is_some()) {
            return None;
        }
        let button = held_button.map_or(3, MouseButton::protocol_code);
        Some(self.encode_mouse(button | 32, false, col, row))
    }

    pub fn mouse_wheel(&self, upward: bool, col: usize, row: usize) -> Option<Vec<u8>> {
        if !self.mouse_tracking_enabled() {
            return None;
        }
        Some(self.encode_mouse(if upward { 64 } else { 65 }, false, col, row))
    }

    /// Return one fixed-width string per screen row, including trailing spaces.
    pub fn render_rows(&self) -> Vec<String> {
        let mut rendered = Vec::with_capacity(self.rows);
        for row in 0..self.rows {
            let start = row * self.cols;
            let end = start + self.cols;
            rendered.push(
                self.cells[start..end]
                    .iter()
                    .map(|cell| cell.glyph)
                    .collect(),
            );
        }
        rendered
    }

    /// Resize while retaining the overlapping top-left portion of the screen.
    pub fn resize(&mut self, cols: usize, rows: usize) {
        let cols = cols.max(1);
        let rows = rows.max(1);
        if cols == self.cols && rows == self.rows {
            return;
        }

        let old_cols = self.cols;
        let old_rows = self.rows;
        let old_cells = core::mem::take(&mut self.cells);
        let mut cells = vec![Cell::blank(); cols.saturating_mul(rows)];
        let copy_cols = old_cols.min(cols);
        let copy_rows = old_rows.min(rows);
        for row in 0..copy_rows {
            let old_start = row * old_cols;
            let new_start = row * cols;
            cells[new_start..new_start + copy_cols]
                .copy_from_slice(&old_cells[old_start..old_start + copy_cols]);
        }

        self.cols = cols;
        self.rows = rows;
        self.cells = cells;
        self.cursor_col = self.cursor_col.min(cols - 1);
        self.cursor_row = self.cursor_row.min(rows - 1);
        self.saved_col = self.saved_col.min(cols - 1);
        self.saved_row = self.saved_row.min(rows - 1);
        self.pending_wrap = false;
        self.scroll_top = 0;
        self.scroll_bottom = rows - 1;
        self.reset_parser();
        self.dirty = true;
    }

    /// Clear the screen and all parser/cursor state without changing its size.
    pub fn reset(&mut self) {
        self.cells.fill(Cell::blank());
        self.current_style = CellStyle::default();
        self.cursor_col = 0;
        self.cursor_row = 0;
        self.saved_col = 0;
        self.saved_row = 0;
        self.cursor_visible = true;
        self.pending_wrap = false;
        self.scroll_top = 0;
        self.scroll_bottom = self.rows - 1;
        self.reset_parser();
        self.responses.clear();
        self.mouse_normal_tracking = false;
        self.mouse_button_tracking = false;
        self.mouse_any_tracking = false;
        self.mouse_sgr_encoding = false;
        self.mouse_urxvt_encoding = false;
        self.dirty = true;
    }

    /// Ingest an arbitrary byte chunk. UTF-8 and escape sequences may span calls.
    pub fn feed(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.feed_byte(byte);
        }
    }

    fn reset_parser(&mut self) {
        self.parser_state = ParserState::Ground;
        self.csi_buf.clear();
        self.csi_overflow = false;
        self.osc_buf.clear();
        self.osc_overflow = false;
        self.utf8_len = 0;
        self.utf8_expected = 0;
    }

    fn feed_byte(&mut self, byte: u8) {
        match self.parser_state {
            ParserState::Ground => self.feed_ground_byte(byte),
            ParserState::Escape => self.feed_escape_byte(byte),
            ParserState::Csi => self.feed_csi_byte(byte),
            ParserState::Osc => match byte {
                0x07 => {
                    self.finish_osc();
                    self.parser_state = ParserState::Ground;
                }
                0x1b => self.parser_state = ParserState::OscEscape,
                _ if self.osc_buf.len() < MAX_OSC_BYTES => self.osc_buf.push(byte),
                _ => self.osc_overflow = true,
            },
            ParserState::OscEscape => match byte {
                b'\\' | 0x07 => {
                    self.finish_osc();
                    self.parser_state = ParserState::Ground;
                }
                0x1b => {}
                _ => self.parser_state = ParserState::Osc,
            },
        }
    }

    fn feed_ground_byte(&mut self, byte: u8) {
        if self.utf8_expected != 0 {
            if byte & 0xc0 == 0x80 {
                self.utf8_buf[self.utf8_len] = byte;
                self.utf8_len += 1;
                if self.utf8_len == self.utf8_expected {
                    let ch = core::str::from_utf8(&self.utf8_buf[..self.utf8_len])
                        .ok()
                        .and_then(|text| text.chars().next())
                        .unwrap_or('\u{fffd}');
                    self.utf8_len = 0;
                    self.utf8_expected = 0;
                    self.put_char(ch);
                }
                return;
            }

            self.utf8_len = 0;
            self.utf8_expected = 0;
            self.put_char('\u{fffd}');
            // The non-continuation byte may itself begin text or a control
            // sequence, so process it again after closing the malformed scalar.
        }

        match byte {
            0x1b => self.parser_state = ParserState::Escape,
            b'\r' => self.carriage_return(),
            b'\n' | 0x0b | 0x0c => self.line_feed(),
            0x08 => self.backspace(),
            b'\t' => self.horizontal_tab(),
            0x00..=0x1f | 0x7f => {}
            0x20..=0x7e => self.put_char(byte as char),
            0xc2..=0xdf => self.begin_utf8(byte, 2),
            0xe0..=0xef => self.begin_utf8(byte, 3),
            0xf0..=0xf4 => self.begin_utf8(byte, 4),
            _ => self.put_char('\u{fffd}'),
        }
    }

    fn begin_utf8(&mut self, byte: u8, expected: usize) {
        self.utf8_buf[0] = byte;
        self.utf8_len = 1;
        self.utf8_expected = expected;
    }

    fn feed_escape_byte(&mut self, byte: u8) {
        self.parser_state = ParserState::Ground;
        match byte {
            b'[' => {
                self.csi_buf.clear();
                self.csi_overflow = false;
                self.parser_state = ParserState::Csi;
            }
            b']' => {
                self.osc_buf.clear();
                self.osc_overflow = false;
                self.parser_state = ParserState::Osc;
            }
            b'7' => {
                self.saved_col = self.cursor_col;
                self.saved_row = self.cursor_row;
            }
            b'8' => self.restore_cursor(),
            b'D' => self.line_feed(),
            b'E' => {
                self.line_feed();
                self.carriage_return();
            }
            b'M' => self.reverse_index(),
            b'c' => self.reset(),
            0x1b => self.parser_state = ParserState::Escape,
            _ => {}
        }
    }

    fn feed_csi_byte(&mut self, byte: u8) {
        match byte {
            0x40..=0x7e => {
                if !self.csi_overflow {
                    self.execute_csi(byte);
                }
                self.csi_buf.clear();
                self.csi_overflow = false;
                self.parser_state = ParserState::Ground;
            }
            0x1b => {
                self.csi_buf.clear();
                self.csi_overflow = false;
                self.parser_state = ParserState::Escape;
            }
            0x18 | 0x1a => {
                self.csi_buf.clear();
                self.csi_overflow = false;
                self.parser_state = ParserState::Ground;
            }
            0x20..=0x3f => {
                if self.csi_buf.len() < MAX_CSI_BYTES {
                    self.csi_buf.push(byte);
                } else {
                    self.csi_overflow = true;
                }
            }
            _ => {}
        }
    }

    fn finish_osc(&mut self) {
        self.osc_buf.clear();
        self.osc_overflow = false;
    }

    fn execute_csi(&mut self, final_byte: u8) {
        let (private, params) = parse_csi_params(&self.csi_buf);
        self.pending_wrap = false;
        match final_byte {
            b'A' => self.move_cursor_vertical(-cursor_count(&params)),
            b'B' => self.move_cursor_vertical(cursor_count(&params)),
            b'C' => self.move_cursor_horizontal(cursor_count(&params)),
            b'D' => self.move_cursor_horizontal(-cursor_count(&params)),
            b'E' => {
                self.move_cursor_vertical(cursor_count(&params));
                self.carriage_return();
            }
            b'F' => {
                self.move_cursor_vertical(-cursor_count(&params));
                self.carriage_return();
            }
            b'G' => {
                let col = position_param(&params, 0);
                self.set_cursor(self.cursor_row, col - 1);
            }
            b'H' | b'f' => {
                let row = position_param(&params, 0);
                let col = position_param(&params, 1);
                self.set_cursor(row - 1, col - 1);
            }
            b'd' => {
                let row = position_param(&params, 0);
                self.set_cursor(row - 1, self.cursor_col);
            }
            b'J' => self.erase_display(params.first().copied().unwrap_or(0)),
            b'K' => self.erase_line(params.first().copied().unwrap_or(0)),
            b'L' => self.insert_lines(count_param(&params)),
            b'M' => self.delete_lines(count_param(&params)),
            b'S' => self.shift_lines_up(self.scroll_top, self.scroll_bottom, count_param(&params)),
            b'T' => {
                self.shift_lines_down(self.scroll_top, self.scroll_bottom, count_param(&params))
            }
            b'r' if !private => self.set_scroll_region(&params),
            b's' => {
                self.saved_col = self.cursor_col;
                self.saved_row = self.cursor_row;
            }
            b'u' => self.restore_cursor(),
            b'h' | b'l' if private => self.set_private_modes(&params, final_byte == b'h'),
            // Device Status Report: answer a cursor-position query using the
            // terminal model's one-based coordinates.
            b'n' if !private && params.first().copied().unwrap_or(0) == 6 => {
                let response = alloc::format!(
                    "\x1b[{};{}R",
                    self.cursor_row.saturating_add(1),
                    self.cursor_col.saturating_add(1)
                );
                self.responses.extend_from_slice(response.as_bytes());
            }
            // Primary Device Attributes. VT100-with-advanced-video is a small,
            // conservative identity understood by terminal applications.
            b'c' if !private => self.responses.extend_from_slice(b"\x1b[?1;2c"),
            b'm' if !private => self.set_graphic_rendition(&params),
            // Remaining device-control sequences do not affect this model.
            b'n' | b'c' | b'q' | b'h' | b'l' => {}
            _ => {}
        }
    }

    fn set_graphic_rendition(&mut self, params: &[usize]) {
        if params.is_empty() {
            self.current_style = CellStyle::default();
            return;
        }

        let mut index = 0;
        while index < params.len() {
            match params[index] {
                0 => self.current_style = CellStyle::default(),
                4 => self.current_style.underline = true,
                24 => self.current_style.underline = false,
                30..=37 => {
                    self.current_style.foreground =
                        TerminalColor::Indexed((params[index] - 30) as u8);
                }
                39 => self.current_style.foreground = TerminalColor::Default,
                40..=47 => {
                    self.current_style.background =
                        TerminalColor::Indexed((params[index] - 40) as u8);
                }
                49 => self.current_style.background = TerminalColor::Default,
                90..=97 => {
                    self.current_style.foreground =
                        TerminalColor::Indexed((params[index] - 90 + 8) as u8);
                }
                100..=107 => {
                    self.current_style.background =
                        TerminalColor::Indexed((params[index] - 100 + 8) as u8);
                }
                38 | 48 => match params.get(index + 1).copied() {
                    Some(5) => {
                        if let Some(color) = params
                            .get(index + 2)
                            .and_then(|value| u8::try_from(*value).ok())
                        {
                            let color = TerminalColor::Indexed(color);
                            if params[index] == 38 {
                                self.current_style.foreground = color;
                            } else {
                                self.current_style.background = color;
                            }
                        }
                        index = index.saturating_add(2);
                    }
                    Some(2) => {
                        let rgb =
                            params
                                .get(index + 2..index + 5)
                                .and_then(|values| match values {
                                    [red, green, blue] => Some((
                                        u8::try_from(*red).ok()?,
                                        u8::try_from(*green).ok()?,
                                        u8::try_from(*blue).ok()?,
                                    )),
                                    _ => None,
                                });
                        if let Some((red, green, blue)) = rgb {
                            let color = TerminalColor::Rgb { red, green, blue };
                            if params[index] == 38 {
                                self.current_style.foreground = color;
                            } else {
                                self.current_style.background = color;
                            }
                        }
                        index = index.saturating_add(4);
                    }
                    _ => {}
                },
                _ => {}
            }
            index = index.saturating_add(1);
        }
    }

    fn set_cursor(&mut self, row: usize, col: usize) {
        self.cursor_row = row.min(self.rows - 1);
        self.cursor_col = col.min(self.cols - 1);
        self.dirty = true;
    }

    fn set_private_modes(&mut self, params: &[usize], enabled: bool) {
        for &mode in params {
            match mode {
                25 => {
                    if self.cursor_visible != enabled {
                        self.cursor_visible = enabled;
                        self.dirty = true;
                    }
                }
                1000 => self.mouse_normal_tracking = enabled,
                1002 => self.mouse_button_tracking = enabled,
                1003 => self.mouse_any_tracking = enabled,
                1006 => self.mouse_sgr_encoding = enabled,
                1015 => self.mouse_urxvt_encoding = enabled,
                _ => {}
            }
        }
    }

    pub fn mouse_tracking_enabled(&self) -> bool {
        self.mouse_normal_tracking || self.mouse_button_tracking || self.mouse_any_tracking
    }

    fn encode_mouse(&self, button_code: u8, release: bool, col: usize, row: usize) -> Vec<u8> {
        let col = col.min(self.cols - 1).saturating_add(1);
        let row = row.min(self.rows - 1).saturating_add(1);
        if self.mouse_sgr_encoding {
            return alloc::format!(
                "\x1b[<{};{};{}{}",
                button_code,
                col,
                row,
                if release { 'm' } else { 'M' }
            )
            .into_bytes();
        }
        if self.mouse_urxvt_encoding {
            let code = if release { 3 } else { button_code };
            return alloc::format!("\x1b[{};{};{}M", code.saturating_add(32), col, row)
                .into_bytes();
        }

        // The original X10 encoding has one byte per coordinate. This shell's
        // fixed grid is comfortably inside its 223-cell representable range.
        let code = if release { 3 } else { button_code };
        vec![
            0x1b,
            b'[',
            b'M',
            code.saturating_add(32),
            (col as u8).saturating_add(32),
            (row as u8).saturating_add(32),
        ]
    }

    fn move_cursor_vertical(&mut self, delta: isize) {
        self.cursor_row = self
            .cursor_row
            .saturating_add_signed(delta)
            .min(self.rows - 1);
        self.dirty = true;
    }

    fn move_cursor_horizontal(&mut self, delta: isize) {
        self.cursor_col = self
            .cursor_col
            .saturating_add_signed(delta)
            .min(self.cols - 1);
        self.dirty = true;
    }

    fn restore_cursor(&mut self) {
        self.cursor_col = self.saved_col.min(self.cols - 1);
        self.cursor_row = self.saved_row.min(self.rows - 1);
        self.pending_wrap = false;
        self.dirty = true;
    }

    fn carriage_return(&mut self) {
        self.cursor_col = 0;
        self.pending_wrap = false;
        self.dirty = true;
    }

    fn backspace(&mut self) {
        self.cursor_col = self.cursor_col.saturating_sub(1);
        self.pending_wrap = false;
        self.dirty = true;
    }

    fn horizontal_tab(&mut self) {
        let next = (self.cursor_col / TAB_WIDTH + 1).saturating_mul(TAB_WIDTH);
        self.cursor_col = next.min(self.cols - 1);
        self.pending_wrap = false;
        self.dirty = true;
    }

    fn line_feed(&mut self) {
        self.pending_wrap = false;
        if self.cursor_row == self.scroll_bottom {
            self.shift_lines_up(self.scroll_top, self.scroll_bottom, 1);
        } else {
            self.cursor_row = self.cursor_row.saturating_add(1).min(self.rows - 1);
            self.dirty = true;
        }
    }

    fn reverse_index(&mut self) {
        self.pending_wrap = false;
        if self.cursor_row == self.scroll_top {
            self.shift_lines_down(self.scroll_top, self.scroll_bottom, 1);
        } else {
            self.cursor_row = self.cursor_row.saturating_sub(1);
            self.dirty = true;
        }
    }

    fn put_char(&mut self, ch: char) {
        if self.pending_wrap {
            self.cursor_col = 0;
            self.line_feed();
        }

        let index = self.cursor_row * self.cols + self.cursor_col;
        self.cells[index] = Cell {
            glyph: ch,
            style: self.current_style,
        };
        if self.cursor_col == self.cols - 1 {
            self.pending_wrap = true;
        } else {
            self.cursor_col += 1;
        }
        self.dirty = true;
    }

    fn erase_display(&mut self, mode: usize) {
        match mode {
            0 => {
                self.clear_line_range(self.cursor_row, self.cursor_col, self.cols);
                for row in self.cursor_row + 1..self.rows {
                    self.clear_line_range(row, 0, self.cols);
                }
            }
            1 => {
                for row in 0..self.cursor_row {
                    self.clear_line_range(row, 0, self.cols);
                }
                self.clear_line_range(self.cursor_row, 0, self.cursor_col + 1);
            }
            2 | 3 => self.clear_all(),
            _ => {}
        }
    }

    fn erase_line(&mut self, mode: usize) {
        match mode {
            0 => self.clear_line_range(self.cursor_row, self.cursor_col, self.cols),
            1 => self.clear_line_range(self.cursor_row, 0, self.cursor_col + 1),
            2 => self.clear_line_range(self.cursor_row, 0, self.cols),
            _ => {}
        }
    }

    fn clear_all(&mut self) {
        let blank = self.erase_blank();
        self.cells.fill(blank);
        self.dirty = true;
    }

    fn clear_line_range(&mut self, row: usize, start: usize, end: usize) {
        if row >= self.rows {
            return;
        }
        let start = start.min(self.cols);
        let end = end.min(self.cols);
        if start >= end {
            return;
        }
        let line_start = row * self.cols;
        let blank = self.erase_blank();
        self.cells[line_start + start..line_start + end].fill(blank);
        self.dirty = true;
    }

    fn erase_blank(&self) -> Cell {
        Cell {
            glyph: ' ',
            style: CellStyle {
                foreground: TerminalColor::Default,
                background: self.current_style.background,
                underline: false,
            },
        }
    }

    fn insert_lines(&mut self, count: usize) {
        if self.cursor_row < self.scroll_top || self.cursor_row > self.scroll_bottom {
            return;
        }
        self.shift_lines_down(self.cursor_row, self.scroll_bottom, count);
    }

    fn delete_lines(&mut self, count: usize) {
        if self.cursor_row < self.scroll_top || self.cursor_row > self.scroll_bottom {
            return;
        }
        self.shift_lines_up(self.cursor_row, self.scroll_bottom, count);
    }

    fn shift_lines_up(&mut self, top: usize, bottom: usize, count: usize) {
        if top > bottom || bottom >= self.rows {
            return;
        }
        let height = bottom - top + 1;
        let count = count.max(1).min(height);
        if count < height {
            let source_start = (top + count) * self.cols;
            let source_end = (bottom + 1) * self.cols;
            let destination = top * self.cols;
            self.cells
                .copy_within(source_start..source_end, destination);
        }
        for row in bottom + 1 - count..=bottom {
            self.clear_line_range(row, 0, self.cols);
        }
        self.dirty = true;
    }

    fn shift_lines_down(&mut self, top: usize, bottom: usize, count: usize) {
        if top > bottom || bottom >= self.rows {
            return;
        }
        let height = bottom - top + 1;
        let count = count.max(1).min(height);
        if count < height {
            let source_start = top * self.cols;
            let source_end = (bottom + 1 - count) * self.cols;
            let destination = (top + count) * self.cols;
            self.cells
                .copy_within(source_start..source_end, destination);
        }
        for row in top..top + count {
            self.clear_line_range(row, 0, self.cols);
        }
        self.dirty = true;
    }

    fn set_scroll_region(&mut self, params: &[usize]) {
        let top = position_param(params, 0);
        let bottom = match params.get(1).copied().unwrap_or(0) {
            0 => self.rows,
            value => value,
        };
        if top <= bottom && bottom <= self.rows {
            self.scroll_top = top - 1;
            self.scroll_bottom = bottom - 1;
        } else {
            self.scroll_top = 0;
            self.scroll_bottom = self.rows - 1;
        }
        self.cursor_col = 0;
        self.cursor_row = 0;
        self.pending_wrap = false;
        self.dirty = true;
    }
}

fn parse_csi_params(raw: &[u8]) -> (bool, Vec<usize>) {
    let private = raw.first() == Some(&b'?');
    let raw = if private { &raw[1..] } else { raw };
    if raw.is_empty() {
        return (private, Vec::new());
    }

    let mut params = Vec::new();
    for field in raw.split(|byte| *byte == b';') {
        let mut value = 0usize;
        let mut valid = !field.is_empty();
        for &byte in field {
            if !byte.is_ascii_digit() {
                valid = false;
                break;
            }
            value = value
                .saturating_mul(10)
                .saturating_add((byte - b'0') as usize);
        }
        params.push(if valid { value } else { 0 });
    }
    (private, params)
}

fn count_param(params: &[usize]) -> usize {
    params.first().copied().unwrap_or(1).max(1)
}

fn cursor_count(params: &[usize]) -> isize {
    count_param(params).min(isize::MAX as usize) as isize
}

fn position_param(params: &[usize], index: usize) -> usize {
    params.get(index).copied().unwrap_or(1).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(terminal: &Terminal) -> Vec<String> {
        terminal.render_rows()
    }

    #[test]
    fn wraps_and_scrolls_at_the_bottom_margin() {
        let mut terminal = Terminal::new(4, 2);
        terminal.feed(b"abcdEFGHx");

        assert_eq!(rows(&terminal), ["EFGH", "x   "]);
        assert_eq!(
            terminal.cursor(),
            Cursor {
                col: 1,
                row: 1,
                visible: true
            }
        );
    }

    #[test]
    fn keeps_utf8_and_csi_state_across_chunks() {
        let mut terminal = Terminal::new(8, 2);
        terminal.feed(&[0xe2]);
        terminal.feed(&[0x98]);
        terminal.feed(&[0x83]);
        terminal.feed(b"abc\x1b[2;");
        terminal.feed(b"3HZ");

        assert_eq!(rows(&terminal), ["\u{2603}abc    ", "  Z     "]);
    }

    #[test]
    fn malformed_utf8_does_not_consume_following_ascii() {
        let mut terminal = Terminal::new(4, 1);
        terminal.feed(&[0xe2, b'X']);

        assert_eq!(rows(&terminal), ["\u{fffd}X  "]);
    }

    #[test]
    fn erases_ranges_without_changing_text_behavior_for_sgr() {
        let mut terminal = Terminal::new(6, 2);
        terminal.feed(b"abcdef\r\n123456\x1b[1;3H\x1b[31m\x1b[K");

        assert_eq!(rows(&terminal), ["ab    ", "123456"]);
    }

    #[test]
    fn records_standard_bright_indexed_and_truecolor_foregrounds() {
        let mut terminal = Terminal::new(8, 1);
        terminal.feed(b"\x1b[30mA\x1b[37mB\x1b[90mC\x1b[97mD\x1b[38;5;202mE\x1b[38;2;1;2;3mF");

        let cells = terminal.cells();
        assert_eq!(cells[0].style.foreground, TerminalColor::Indexed(0));
        assert_eq!(cells[1].style.foreground, TerminalColor::Indexed(7));
        assert_eq!(cells[2].style.foreground, TerminalColor::Indexed(8));
        assert_eq!(cells[3].style.foreground, TerminalColor::Indexed(15));
        assert_eq!(cells[4].style.foreground, TerminalColor::Indexed(202));
        assert_eq!(
            cells[5].style.foreground,
            TerminalColor::Rgb {
                red: 1,
                green: 2,
                blue: 3
            }
        );
    }

    #[test]
    fn resets_foreground_with_default_empty_and_full_reset_sgr() {
        let mut terminal = Terminal::new(6, 1);
        terminal.feed(b"\x1b[31mA\x1b[39mB\x1b[32m\x1b[mC\x1b[33m\x1b[0mD");

        assert_eq!(
            terminal.cells()[0].style.foreground,
            TerminalColor::Indexed(1)
        );
        for cell in &terminal.cells()[1..4] {
            assert_eq!(cell.style.foreground, TerminalColor::Default);
        }
    }

    #[test]
    fn records_standard_bright_indexed_and_truecolor_backgrounds() {
        let mut terminal = Terminal::new(8, 1);
        terminal.feed(b"\x1b[40mA\x1b[47mB\x1b[100mC\x1b[107mD\x1b[48;5;202mE\x1b[48;2;1;2;3mF");

        let cells = terminal.cells();
        assert_eq!(cells[0].style.background, TerminalColor::Indexed(0));
        assert_eq!(cells[1].style.background, TerminalColor::Indexed(7));
        assert_eq!(cells[2].style.background, TerminalColor::Indexed(8));
        assert_eq!(cells[3].style.background, TerminalColor::Indexed(15));
        assert_eq!(cells[4].style.background, TerminalColor::Indexed(202));
        assert_eq!(
            cells[5].style.background,
            TerminalColor::Rgb {
                red: 1,
                green: 2,
                blue: 3
            }
        );
    }

    #[test]
    fn resets_background_with_default_empty_and_full_reset_sgr() {
        let mut terminal = Terminal::new(6, 1);
        terminal.feed(b"\x1b[44mA\x1b[49mB\x1b[45m\x1b[mC\x1b[46m\x1b[0mD");

        assert_eq!(
            terminal.cells()[0].style.background,
            TerminalColor::Indexed(4)
        );
        for cell in &terminal.cells()[1..4] {
            assert_eq!(cell.style.background, TerminalColor::Default);
        }
    }

    #[test]
    fn erased_cells_keep_the_current_background() {
        let mut terminal = Terminal::new(6, 1);
        terminal.feed(b"abcdef\x1b[1G\x1b[48;2;9;8;7m\x1b[2K");

        for cell in terminal.cells() {
            assert_eq!(
                cell.style.background,
                TerminalColor::Rgb {
                    red: 9,
                    green: 8,
                    blue: 7
                }
            );
        }
    }

    #[test]
    fn inserts_and_deletes_lines_inside_scroll_region() {
        let mut terminal = Terminal::new(3, 4);
        terminal.feed(b"aaa\r\nbbb\r\nccc\r\nddd");
        terminal.feed(b"\x1b[2;4r\x1b[3;1H\x1b[L");
        assert_eq!(rows(&terminal), ["aaa", "bbb", "   ", "ccc"]);

        terminal.feed(b"\x1b[M");
        assert_eq!(rows(&terminal), ["aaa", "bbb", "ccc", "   "]);
    }

    #[test]
    fn ignores_osc_terminated_by_bel_or_split_st() {
        let mut terminal = Terminal::new(8, 1);
        terminal.feed(b"A\x1b]0;title\x07B\x1b]777;ignored\x1b");
        terminal.feed(b"\\C");

        assert_eq!(rows(&terminal), ["ABC     "]);
    }

    #[test]
    fn tracks_cursor_visibility_save_and_restore() {
        let mut terminal = Terminal::new(5, 2);
        terminal.feed(b"ab\x1b[s\x1b[2;5H\x1b[?25lX\x1b[u");

        assert_eq!(
            terminal.cursor(),
            Cursor {
                col: 2,
                row: 0,
                visible: false
            }
        );
        terminal.feed(b"\x1b[?25h");
        assert!(terminal.cursor().visible);
    }

    #[test]
    fn resize_preserves_the_overlapping_cells() {
        let mut terminal = Terminal::new(4, 2);
        terminal.feed(b"ab\r\ncd");
        assert!(terminal.take_dirty());
        assert!(!terminal.is_dirty());

        terminal.resize(3, 3);
        assert_eq!(terminal.dimensions(), (3, 3));
        assert_eq!(rows(&terminal), ["ab ", "cd ", "   "]);
        assert!(terminal.take_dirty());

        terminal.reset();
        assert_eq!(rows(&terminal), ["   ", "   ", "   "]);
    }

    #[test]
    fn answers_cursor_and_device_attribute_queries() {
        let mut terminal = Terminal::new(8, 2);
        terminal.feed("\r…\x1b[6n\x1b[c".as_bytes());

        assert_eq!(terminal.take_responses(), b"\x1b[1;2R\x1b[?1;2c");
        assert!(terminal.take_responses().is_empty());
    }

    #[test]
    fn reports_sgr_mouse_only_when_the_application_requests_it() {
        let mut terminal = Terminal::new(100, 27);
        assert_eq!(terminal.mouse_button(MouseButton::Left, true, 4, 2), None);

        terminal.feed(b"\x1b[?1000h\x1b[?1002h\x1b[?1015h\x1b[?1006h");
        assert_eq!(
            terminal.mouse_button(MouseButton::Left, true, 4, 2),
            Some(b"\x1b[<0;5;3M".to_vec())
        );
        assert_eq!(
            terminal.mouse_button(MouseButton::Right, false, 4, 2),
            Some(b"\x1b[<2;5;3m".to_vec())
        );
        assert_eq!(
            terminal.mouse_motion(Some(MouseButton::Middle), 4, 2),
            Some(b"\x1b[<33;5;3M".to_vec())
        );
        assert_eq!(
            terminal.mouse_wheel(true, 4, 2),
            Some(b"\x1b[<64;5;3M".to_vec())
        );

        terminal.feed(b"\x1b[?1006l\x1b[?1015l\x1b[?1002l\x1b[?1000l");
        assert_eq!(terminal.mouse_button(MouseButton::Left, true, 4, 2), None);
    }

    #[test]
    fn any_motion_and_button_motion_modes_remain_distinct() {
        let mut terminal = Terminal::new(8, 4);
        terminal.feed(b"\x1b[?1002h\x1b[?1006h");
        assert_eq!(terminal.mouse_motion(None, 1, 1), None);
        assert!(
            terminal
                .mouse_motion(Some(MouseButton::Left), 1, 1)
                .is_some()
        );

        terminal.feed(b"\x1b[?1003h");
        assert_eq!(
            terminal.mouse_motion(None, 1, 1),
            Some(b"\x1b[<35;2;2M".to_vec())
        );
    }
}
