use alloc::vec::Vec;
const UI3_SHELL_DEFAULT_FG: (u8, u8, u8) = (0xF1, 0xF4, 0xF8);
const UI3_SHELL_DEFAULT_BG: (u8, u8, u8) = (0x0C, 0x10, 0x16);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Ui3ShellCell {
    pub ch: char,
    pub fg: (u8, u8, u8),
    pub bg: (u8, u8, u8),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Ui3ShellScreenSnapshot {
    pub cols: u32,
    pub rows: u32,
    pub cursor_col: u32,
    pub cursor_row: u32,
    pub cursor_visible: bool,
    pub terminal_handoff: bool,
    pub cells: Vec<Ui3ShellCell>,
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
            fg: UI3_SHELL_DEFAULT_FG,
            bg: UI3_SHELL_DEFAULT_BG,
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

pub(crate) struct TerminalState {
    cols: usize,
    rows: usize,
    cursor_col: usize,
    cursor_row: usize,
    saved_col: usize,
    saved_row: usize,
    pending_wrap: bool,
    scroll_top: usize,
    scroll_bottom: usize,
    cursor_visible: bool,
    style: TerminalStyle,
    cells: Vec<Ui3ShellCell>,
    esc_state: EscapeState,
    csi_buf: Vec<u8>,
    osc_buf: Vec<u8>,
    utf8_buf: [u8; 4],
    utf8_len: usize,
    utf8_expected: usize,
    terminal_handoff: bool,
}

impl TerminalState {
    pub(crate) fn new(cols: usize, rows: usize) -> Self {
        let mut out = Self {
            cols: cols.max(1),
            rows: rows.max(1),
            cursor_col: 0,
            cursor_row: 0,
            saved_col: 0,
            saved_row: 0,
            pending_wrap: false,
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
            terminal_handoff: false,
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
        self.pending_wrap = false;
        self.style = TerminalStyle::default();
        self.cells = vec![Self::blank_cell(); self.cols.saturating_mul(self.rows)];
        self.esc_state = EscapeState::None;
        self.csi_buf.clear();
        self.osc_buf.clear();
        self.utf8_len = 0;
        self.utf8_expected = 0;
        self.terminal_handoff = false;
    }

    pub(crate) fn resize_preserving_contents(&mut self, cols: usize, rows: usize) {
        let next_cols = cols.max(1);
        let next_rows = rows.max(1);
        if self.cols == next_cols && self.rows == next_rows {
            return;
        }

        let old_cols = self.cols;
        let old_rows = self.rows;
        let old_cells = core::mem::take(&mut self.cells);
        let mut next_cells = vec![Self::blank_cell(); next_cols.saturating_mul(next_rows)];
        let copy_rows = old_rows.min(next_rows);
        let copy_cols = old_cols.min(next_cols);
        for row in 0..copy_rows {
            let old_start = row.saturating_mul(old_cols);
            let next_start = row.saturating_mul(next_cols);
            let old_end = old_start.saturating_add(copy_cols).min(old_cells.len());
            let next_end = next_start.saturating_add(copy_cols).min(next_cells.len());
            if old_end <= old_start || next_end <= next_start {
                continue;
            }
            next_cells[next_start..next_end].copy_from_slice(&old_cells[old_start..old_end]);
        }

        let had_full_scroll_region =
            self.scroll_top == 0 && self.scroll_bottom >= old_rows.saturating_sub(1);

        self.cols = next_cols;
        self.rows = next_rows;
        self.cells = next_cells;
        self.cursor_col = self.cursor_col.min(self.cols.saturating_sub(1));
        self.cursor_row = self.cursor_row.min(self.rows.saturating_sub(1));
        self.saved_col = self.saved_col.min(self.cols.saturating_sub(1));
        self.saved_row = self.saved_row.min(self.rows.saturating_sub(1));
        self.pending_wrap = false;
        if had_full_scroll_region {
            self.scroll_top = 0;
            self.scroll_bottom = self.rows.saturating_sub(1);
        } else {
            self.scroll_top = self.scroll_top.min(self.rows.saturating_sub(1));
            self.scroll_bottom = self.scroll_bottom.min(self.rows.saturating_sub(1));
            if self.scroll_bottom < self.scroll_top {
                self.scroll_bottom = self.scroll_top;
            }
        }
    }

    pub(crate) fn snapshot(&self) -> Ui3ShellScreenSnapshot {
        Ui3ShellScreenSnapshot {
            cols: self.cols as u32,
            rows: self.rows as u32,
            cursor_col: self.cursor_col as u32,
            cursor_row: self.cursor_row as u32,
            cursor_visible: self.cursor_visible,
            terminal_handoff: self.terminal_handoff,
            cells: self.cells.clone(),
        }
    }

    fn blank_cell() -> Ui3ShellCell {
        Ui3ShellCell {
            ch: ' ',
            fg: UI3_SHELL_DEFAULT_FG,
            bg: UI3_SHELL_DEFAULT_BG,
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
        self.cells[idx] = Ui3ShellCell { ch, fg, bg };
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
        let region_height = self
            .scroll_bottom
            .saturating_sub(self.scroll_top)
            .saturating_add(1);
        let count = count.max(1).min(region_height);
        if count >= region_height {
            for row in self.scroll_top..=self.scroll_bottom {
                self.clear_line_range(row, 0, self.cols.saturating_sub(1));
            }
            return;
        }

        let last_dst_row = self.scroll_bottom - count;
        for row in self.scroll_top..=last_dst_row {
            for col in 0..self.cols {
                let dst = self.cell_index(row, col);
                let src = self.cell_index(row + count, col);
                self.cells[dst] = self.cells[src];
            }
        }
        for row in last_dst_row.saturating_add(1)..=self.scroll_bottom {
            self.clear_line_range(row, 0, self.cols.saturating_sub(1));
        }
    }

    fn insert_lines(&mut self, count: usize) {
        if self.cursor_row < self.scroll_top || self.cursor_row > self.scroll_bottom {
            return;
        }
        let region_height = self
            .scroll_bottom
            .saturating_sub(self.cursor_row)
            .saturating_add(1);
        let count = count.max(1).min(region_height);
        if count >= region_height {
            for row in self.cursor_row..=self.scroll_bottom {
                self.clear_line_range(row, 0, self.cols.saturating_sub(1));
            }
            return;
        }

        let last_src_row = self.scroll_bottom - count;
        for row in (self.cursor_row..=last_src_row).rev() {
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
            '\r' => {
                self.pending_wrap = false;
                self.cursor_col = 0;
            }
            '\n' => {
                self.pending_wrap = false;
                self.line_feed();
            }
            '\u{0008}' => {
                self.pending_wrap = false;
                self.cursor_col = self.cursor_col.saturating_sub(1);
            }
            '\t' => {
                self.pending_wrap = false;
                let next_tab = ((self.cursor_col / 8).saturating_add(1)).saturating_mul(8);
                self.cursor_col = next_tab.min(self.cols.saturating_sub(1));
            }
            _ => {
                if self.pending_wrap {
                    self.cursor_col = 0;
                    self.line_feed();
                    self.pending_wrap = false;
                }
                self.set_cell(self.cursor_row, self.cursor_col, ch);
                if self.cursor_col >= self.cols.saturating_sub(1) {
                    self.pending_wrap = true;
                } else {
                    self.cursor_col = self.cursor_col.saturating_add(1);
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
                    self.exec_osc();
                    self.osc_buf.clear();
                    self.esc_state = EscapeState::None;
                } else {
                    self.osc_buf.push(b);
                }
            }
        }
    }

    fn exec_osc(&mut self) {
        let Ok(raw) = core::str::from_utf8(self.osc_buf.as_slice()) else {
            return;
        };
        match raw {
            "777;terminal_handoff=1" => self.set_terminal_handoff(true),
            "777;terminal_handoff=0" => self.set_terminal_handoff(false),
            _ => {
                if let Some((row, col, bytes)) = parse_konsole_row_osc(raw) {
                    self.feed_konsole_row(row, col, bytes.as_slice());
                }
            }
        }
    }

    fn feed_konsole_row(&mut self, row: usize, col: usize, bytes: &[u8]) {
        if row >= self.rows || col >= self.cols {
            return;
        }
        let saved_col = self.cursor_col;
        let saved_row = self.cursor_row;
        let saved_visible = self.cursor_visible;
        let saved_style = self.style;
        let saved_scroll_top = self.scroll_top;
        let saved_scroll_bottom = self.scroll_bottom;
        self.esc_state = EscapeState::None;
        self.csi_buf.clear();
        self.osc_buf.clear();
        self.utf8_len = 0;
        self.utf8_expected = 0;
        self.pending_wrap = false;
        self.style = TerminalStyle::default();
        self.cursor_row = row;
        self.cursor_col = col;
        self.clear_line_range(row, col, self.cols.saturating_sub(1));
        self.feed_bytes(bytes);
        self.esc_state = EscapeState::None;
        self.csi_buf.clear();
        self.osc_buf.clear();
        self.utf8_len = 0;
        self.utf8_expected = 0;
        self.pending_wrap = false;
        self.style = saved_style;
        self.scroll_top = saved_scroll_top;
        self.scroll_bottom = saved_scroll_bottom;
        self.cursor_col = saved_col.min(self.cols.saturating_sub(1));
        self.cursor_row = saved_row.min(self.rows.saturating_sub(1));
        self.cursor_visible = saved_visible;
    }

    pub(crate) fn set_terminal_handoff(&mut self, enabled: bool) {
        if self.terminal_handoff == enabled {
            return;
        }
        self.terminal_handoff = enabled;
        if enabled {
            self.cursor_visible = false;
            self.cursor_col = 0;
            self.cursor_row = 0;
            self.clear_all();
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
        self.pending_wrap = false;
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
                let bottom_param = params.get(1).copied().unwrap_or(self.rows as i32);
                let bottom = if bottom_param <= 0 {
                    self.rows
                } else {
                    bottom_param as usize
                };
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
                39 => self.style.fg = UI3_SHELL_DEFAULT_FG,
                40..=47 => self.style.bg = ansi_basic_rgb((codes[idx] - 40) as u8),
                49 => self.style.bg = UI3_SHELL_DEFAULT_BG,
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

    pub(crate) fn feed_bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.feed_byte(b);
        }
    }
}

fn parse_konsole_row_osc(raw: &str) -> Option<(usize, usize, Vec<u8>)> {
    let value = raw.strip_prefix("777;konsole_row=")?;
    let (coords, hex) = value.split_once(';')?;
    let (row, col) = coords.split_once(',')?;
    let row = row.parse::<usize>().ok()?;
    let col = col.parse::<usize>().ok()?;
    if hex.len() % 2 != 0 {
        return None;
    }
    let mut bytes = Vec::new();
    for pair in hex.as_bytes().chunks_exact(2) {
        let high = hex_nibble(pair[0])?;
        let low = hex_nibble(pair[1])?;
        bytes.push((high << 4) | low);
    }
    Some((row, col, bytes))
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
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
