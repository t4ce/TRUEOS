use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;
use core::cell::RefCell;
use crate::shell::{ShellBackend, ShellIo};

#[inline]
pub(crate) fn output_bottom_row(term_rows: usize) -> usize {
    core::cmp::max(3, term_rows.saturating_sub(1))
}

pub(crate) struct ReverseOutput<'a> {
    inner: &'a dyn ShellBackend,
    term_cols: usize,
    term_rows: usize,
    line_buf: RefCell<String>,
}

impl<'a> ReverseOutput<'a> {
    pub(crate) fn new(inner: &'a dyn ShellBackend, term_cols: usize, term_rows: usize) -> Self {
        Self { 
            inner, 
            term_cols,
            term_rows,
            line_buf: RefCell::new(String::new()),
        }
    }

    fn do_write(&self, s: &str) {
        let mut buf = self.line_buf.borrow_mut();
        
        if !s.contains('\n') {
            buf.push_str(s);
            return;
        }

        let parts: Vec<&str> = s.split('\n').collect();
        for (i, part) in parts.iter().enumerate() {
            if i == 0 {
                // First part: append to buffer and flush
                buf.push_str(part);
                if !buf.is_empty() {
                    self.flush_line(&buf);
                    buf.clear();
                }
            } else if i == parts.len() - 1 {
                // Last part: just append to buffer, wait for next newline
                buf.push_str(part);
            } else {
                // Middle parts: flush directly
                self.flush_line(part);
            }
        }
    }
    
    fn flush_line(&self, s: &str) {
        let bottom = output_bottom_row(self.term_rows);
        
        // 1. Insert Line at Row 3
        self.inner.write_fmt(format_args!("{}", crate::ecma48::pos(3, 1)));
        self.inner.write_str("\x1b[L");

        // 2. Repair Scrollbar (Column 1)
        // Row 4 (just pushed down) needs repair
        self.inner.write_fmt(format_args!("{}", crate::ecma48::pos(4, 1)));
        self.inner.write_str("\x1b[38;2;80;80;80m│\x1b[0m");
        // Bottom row connection
        self.inner.write_fmt(format_args!("{}", crate::ecma48::pos(bottom, 1)));
        self.inner.write_str("\x1b[38;2;80;80;80m┴\x1b[0m");

        // 3. Draw new Top Scrollbar join
        self.inner.write_fmt(format_args!("{}", crate::ecma48::pos(3, 1)));
        self.inner.write_str("\x1b[38;2;80;80;80m┬\x1b[0m");
        
        // 4. Output the Content (Right Aligned, Blue)
        self.inner.write_str(" "); // Space separator

        let content_len = s.chars().count();
        let max_width = self.term_cols.saturating_sub(2);
        let padding = max_width.saturating_sub(content_len);
        
        for _ in 0..padding {
            self.inner.write_str(" ");
        }
        
        // Clock/System Message color: (120, 210, 255)
        self.inner.write_str("\x1b[38;2;120;210;255m");
        self.inner.write_str(s);
        self.inner.write_str("\x1b[0m");
    }

    pub(crate) fn echo_command(&self, cmd: &str) {
        if cmd.is_empty() { return; }
        
        // Ensure pending buffer is handled? 
        // For echo_command, we force flush any partial line? 
        // Or assume it's separate? Original code didn't check.
        
        let bottom = output_bottom_row(self.term_rows);
        
        self.inner.write_fmt(format_args!("{}", crate::ecma48::pos(3, 1)));
        self.inner.write_str("\x1b[L");
        
        self.inner.write_fmt(format_args!("{}", crate::ecma48::pos(4, 1)));
        self.inner.write_str("\x1b[38;2;80;80;80m│\x1b[0m");
        self.inner.write_fmt(format_args!("{}", crate::ecma48::pos(bottom, 1)));
        self.inner.write_str("\x1b[38;2;80;80;80m┴\x1b[0m");

        self.inner.write_fmt(format_args!("{}", crate::ecma48::pos(3, 1)));
        self.inner.write_str("\x1b[38;2;80;80;80m┬\x1b[0m");
        self.inner.write_str(" \x1b[37m"); // White
        self.inner.write_str(cmd);
        self.inner.write_str("\x1b[0m");
    }

    pub(crate) fn write_overlay_hint(&self, text: &str) {
        if self.term_cols == 0 || self.term_rows < 3 {
            return;
        }
        // if text.is_empty() { return; } // Allow clearing? Original checked is_empty.

        if text.is_empty() {
             return;
        }

        // Clip text
        let reserved_cols = 2usize;
        let max_text_cols = self.term_cols.saturating_sub(reserved_cols);

        let mut clipped = String::new();
        let mut cols = 0usize;
        for ch in text.chars() {
            if cols >= max_text_cols {
                break;
            }
            clipped.push(ch);
            cols += 1;
        }

        self.inner.write_str(crate::ecma48::SAVE_CURSOR);
        let bottom = output_bottom_row(self.term_rows);
        
        self.inner.write_fmt(format_args!("{}", crate::ecma48::pos(3, 1)));
        self.inner.write_str("\x1b[L"); 
        
        // Repair
        self.inner.write_fmt(format_args!("{}", crate::ecma48::pos(4, 1)));
        self.inner.write_str("\x1b[38;2;80;80;80m│\x1b[0m");
        self.inner.write_fmt(format_args!("{}", crate::ecma48::pos(bottom, 1)));
        self.inner.write_str("\x1b[38;2;80;80;80m┴\x1b[0m");

        self.inner.write_fmt(format_args!("{}", crate::ecma48::pos(3, 1)));
        self.inner.write_str("\x1b[38;2;80;80;80m┬\x1b[0m ");
        self.inner.write_str(clipped.as_str());
        
        self.inner.write_str(crate::ecma48::RESTORE_CURSOR);
    }
}

impl<'a> Drop for ReverseOutput<'a> {
    fn drop(&mut self) {
        let buf = self.line_buf.borrow();
        if !buf.is_empty() {
             self.flush_line(&buf);
        }
    }
}

impl ShellIo for ReverseOutput<'_> {
    fn write_str(&self, s: &str) {
        self.do_write(s);
    }

    fn write_fmt(&self, args: core::fmt::Arguments<'_>) {
        struct Adapter<'a>(&'a ReverseOutput<'a>);
        impl<'a> Write for Adapter<'a> {
            fn write_str(&mut self, s: &str) -> core::fmt::Result {
                self.0.write_str(s);
                Ok(())
            }
        }
        let _ = Adapter(self).write_fmt(args);
    }

    fn write_char(&self, ch: char) {
        let mut b = [0u8; 4];
        self.write_str(ch.encode_utf8(&mut b));
    }

    fn write_byte(&self, b: u8) {
        if b == b'\n' {
            self.write_str("\n");
        } else {
             let mut buf = [0u8; 1];
             buf[0] = b;
             if let Ok(s) = core::str::from_utf8(&buf) {
                 self.write_str(s);
             }
        }
    }
}

impl ShellBackend for ReverseOutput<'_> {
    fn read_byte(&self) -> Option<u8> {
        self.inner.read_byte()
    }
    fn init(&self) {
        self.inner.init()
    }
}


// Independent helpers

pub(crate) fn apply_shell_scroll_region(io: &dyn ShellIo, term_rows: usize) {
    let top = 3usize;
    let bottom = output_bottom_row(term_rows);
    // HIDE cursor to prevent "jump to 1,1" artifact during DECSTBM.
    io.write_str(crate::ecma48::HIDE_CURSOR);
    io.write_fmt(format_args!("\x1b[{};{}r", top, bottom));
    
    // Draw dummy scrollbar static indicator for the entire scroll region.
    io.write_str("\x1b[38;2;80;80;80m"); // Dim Gray
    
    // Top glyph
    io.write_fmt(format_args!("{}", crate::ecma48::pos(top, 1)));
    io.write_str("┬");
    
    // Middle glyphs
    for r in (top + 1)..bottom {
        io.write_fmt(format_args!("{}", crate::ecma48::pos(r, 1)));
        io.write_str("│");
    }
    
    // Dummy percent indicator (approximate center)
    let center = (top + bottom) / 2;
    io.write_fmt(format_args!("{}", crate::ecma48::pos(center, 1)));
    io.write_str("\x1b[38;2;150;150;150m");
    io.write_str("%");
    io.write_str("\x1b[38;2;80;80;80m"); 
    
    // Bottom glyph
    io.write_fmt(format_args!("{}", crate::ecma48::pos(bottom, 1)));
    io.write_str("┴");

    io.write_str("\x1b[0m"); // Reset

    // Immediately force cursor to (3,3) (Top of region, +2 margin for scrollbar + space)
    io.write_fmt(format_args!("{}", crate::ecma48::pos(3, 3)));
    
    io.write_str(crate::ecma48::SHOW_CURSOR);
}
