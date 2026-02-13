use super::matrix;

pub const INDICATOR_COUNT: usize = matrix::STATUS_INDICATORS;
pub const TEXT_WIDTH: usize = matrix::STATUS_TEXT_LEN;

pub type IndicatorCode = u8;

pub use super::matrix::StatusBarSnapshot;

#[inline]
pub fn set_active_slot(slot_id: u8) -> bool {
    matrix::set_active_status_slot(slot_id)
}

#[inline]
pub fn active_slot() -> Option<u8> {
    matrix::active_status_slot()
}

#[inline]
pub fn clear_active_slot() {
    matrix::clear_active_status_slot()
}

#[inline]
pub fn set_left(slot_id: u8, text: &str) -> bool {
    matrix::status_set_left(slot_id, text)
}

#[inline]
pub fn set_right(slot_id: u8, text: &str) -> bool {
    matrix::status_set_right(slot_id, text)
}

#[inline]
pub fn set_indicator(slot_id: u8, index: usize, color_code: IndicatorCode) -> bool {
    matrix::status_set_indicator(slot_id, index, color_code)
}

#[inline]
pub fn set_left_active(text: &str) -> bool {
    matrix::status_set_left_active(text)
}

#[inline]
pub fn set_right_active(text: &str) -> bool {
    matrix::status_set_right_active(text)
}

#[inline]
pub fn set_indicator_active(index: usize, color_code: IndicatorCode) -> bool {
    matrix::status_set_indicator_active(index, color_code)
}

#[inline]
pub fn snapshot_active() -> Option<StatusBarSnapshot> {
    matrix::active_status_snapshot()
}

pub fn refresh(io: &dyn crate::shell::ShellIo, term_cols: usize, term_rows: usize) {
    if term_cols == 0 || term_rows == 0 {
        return;
    }

    #[inline]
    fn indicator_rgb(code: u8) -> (u8, u8, u8) {
        match code {
            1 => (230, 70, 70),   // red
            2 => (70, 210, 90),   // green
            3 => (230, 190, 70),  // yellow
            4 => (70, 130, 230),  // blue
            5 => (80, 200, 210),  // cyan
            6 => (200, 90, 210),  // magenta
            7 => (210, 210, 210), // white
            _ => (90, 90, 90),    // off/idle
        }
    }

    fn fit_10(src: &str) -> heapless::String<10> {
        let mut out: heapless::String<10> = heapless::String::new();
        for ch in src.chars() {
            if out.push(ch).is_err() {
                break;
            }
        }
        while out.len() < 10 {
            let _ = out.push(' ');
        }
        out
    }

    fn fit_10_right(src: &str) -> heapless::String<10> {
        let mut out: heapless::String<10> = heapless::String::new();
        let mut len = 0;
        for _ in src.chars() {
            len += 1;
            if len >= 10 { break; }
        }
        
        for _ in 0..(10 - len) {
            let _ = out.push(' ');
        }
        for ch in src.chars() {
            if out.push(ch).is_err() {
                break;
            }
        }
        out
    }

    let (indicators, left, right) = if let Some(s) = snapshot_active() {
        (
            s.indicators,
            fit_10(s.left.as_str()),
            fit_10_right(s.right.as_str()),
        )
    } else {
        (
            [0u8; INDICATOR_COUNT],
            fit_10(""),
            fit_10_right(""),
        )
    };

    let left: heapless::String<10> = left;
    let right: heapless::String<10> = right;

    let right_col = term_cols.saturating_sub(10).saturating_add(1);
    let left_col = 1usize;

    io.write_str(crate::ecma48::HIDE_CURSOR);
    io.write_str(crate::ecma48::SAVE_CURSOR);
    io.write_fmt(format_args!("{}", crate::ecma48::pos(term_rows, 1)));

    // White background for status bar
    let bar_bg = (255u8, 255u8, 255u8);
    io.write_fmt(format_args!("\x1b[48;2;{};{};{}m", bar_bg.0, bar_bg.1, bar_bg.2));
    for _ in 0..term_cols {
        io.write_byte(b' ');
    }
    io.write_str(crate::ecma48::RESET);

    io.write_fmt(format_args!("{}", crate::ecma48::pos(term_rows, left_col)));
    for c in indicators {
        // Adjust indicator color 7 (white) to be dark so it's visible on white bg
        let fg = if c == 7 { (0u8, 0u8, 0u8) } else { indicator_rgb(c) };
        io.write_fmt(format_args!("{}", crate::ecma48::style("o").fg(fg).bg(bar_bg)));
    }
    io.write_fmt(format_args!("{}", crate::ecma48::style(" ").bg(bar_bg)));
    
    // Left text: dark grey on white
    io.write_fmt(format_args!("{}", crate::ecma48::style(left.as_str()).fg((50, 50, 50)).bg(bar_bg)));

    io.write_fmt(format_args!("{}", crate::ecma48::pos(term_rows, right_col)));
    // Right text: darker pink on white
    io.write_fmt(format_args!("{}", crate::ecma48::style(right.as_str()).bold().fg((200, 50, 150)).bg(bar_bg)));

    io.write_str(crate::ecma48::RESTORE_CURSOR);
    io.write_str(crate::ecma48::SHOW_CURSOR);
}

