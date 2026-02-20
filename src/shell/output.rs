use crate::shell::{ShellBackend, ShellIo};
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;
use core::fmt::Write;

#[inline]
pub(crate) fn output_bottom_row(term_rows: usize) -> usize {
    core::cmp::max(3, term_rows.saturating_sub(1))
}

pub(crate) struct ReverseOutput<'a> {
    inner: &'a dyn ShellBackend,
    term_cols: usize,
    term_rows: usize,
    line_buf: RefCell<String>,
    live_line_inserted: RefCell<bool>,
    history: RefCell<&'a mut Vec<String>>,
}

impl<'a> ReverseOutput<'a> {
    pub(crate) fn new(
        inner: &'a dyn ShellBackend,
        term_cols: usize,
        term_rows: usize,
        history: &'a mut Vec<String>,
    ) -> Self {
        Self {
            inner,
            term_cols,
            term_rows,
            line_buf: RefCell::new(String::new()),
            live_line_inserted: RefCell::new(false),
            history: RefCell::new(history),
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

    fn render_preview_line(&self, s: &str) {
        let s = s.trim_end_matches('\r');
        let bottom = output_bottom_row(self.term_rows);
        let max_width = self.term_cols.saturating_sub(2);

        self.inner.write_str(crate::ecma48::SAVE_CURSOR);
        self.inner.write_str(crate::ecma48::HIDE_CURSOR);
        // Render previews into the scrollback area (row 3), not on the prompt row.
        // If we render on the prompt row, the next prompt redraw will erase it, which
        // looks like "no output" for commands that stream progress (notably `update`).
        self.inner
            .write_fmt(format_args!("{}", crate::ecma48::pos(3, 2))); // Col 2 (skip scrollbar)
        self.inner.write_str("\x1b[K");

        let content_len = crate::shell::ecma48::visible_width(s);
        let padding = max_width.saturating_sub(content_len);
        for _ in 0..padding {
            self.inner.write_str(" ");
        }

        let final_s = if content_len > max_width {
            let mut clipped = String::new();
            let mut w = 0usize;
            for ch in s.chars() {
                w += 1;
                if w > max_width {
                    break;
                }
                clipped.push(ch);
            }
            clipped
        } else {
            String::from(s)
        };

        self.inner.write_str("\x1b[38;2;120;210;255m");
        self.inner.write_str(final_s.as_str());
        self.inner.write_str("\x1b[0m");

        draw_scrollbar(
            self.inner,
            self.history.borrow().len(),
            bottom.saturating_sub(3),
            0,
            3,
            bottom,
        );
        self.inner.write_str(crate::ecma48::RESTORE_CURSOR);
        self.inner.write_str(crate::ecma48::SHOW_CURSOR);
    }

    /// Live stream path: render partial text immediately without committing a history line.
    /// Commit still happens when a trailing newline is written through `write_str`.
    pub(crate) fn write_live_fragment(&self, s: &str) {
        if s.is_empty() {
            return;
        }
        if !*self.live_line_inserted.borrow() {
            let bottom = output_bottom_row(self.term_rows);
            // Ensure the insert-line happens in the scrollback area.
            self.inner
                .write_fmt(format_args!("{}", crate::ecma48::pos(3, 1)));
            self.inner.write_str("\x1b[L");
            draw_scrollbar(
                self.inner,
                self.history.borrow().len(),
                bottom.saturating_sub(3),
                0,
                3,
                bottom,
            );
            *self.live_line_inserted.borrow_mut() = true;
        }
        let preview = {
            let mut buf = self.line_buf.borrow_mut();
            buf.push_str(s);
            String::from(buf.as_str())
        };
        self.render_preview_line(preview.as_str());
    }

    /// Live stream path with soft-wrap to keep long, no-newline output visible.
    pub(crate) fn write_live_fragment_wrapped(&self, s: &str) {
        if s.is_empty() {
            return;
        }
        let max_width = self.term_cols.saturating_sub(2).max(1);
        let mut col = {
            let buf = self.line_buf.borrow();
            crate::shell::ecma48::visible_width(buf.as_str())
        };

        let mut seg = String::new();
        let mut seg_w = 0usize;

        for ch in s.chars() {
            if ch == '\n' {
                if !seg.is_empty() {
                    self.write_live_fragment(seg.as_str());
                    col = col.saturating_add(seg_w);
                    seg.clear();
                    seg_w = 0;
                }
                self.write_str("\n");
                col = 0;
                continue;
            }

            let mut enc = [0u8; 4];
            let ch_s = ch.encode_utf8(&mut enc);
            let ch_w = crate::shell::ecma48::visible_width(ch_s).max(1);

            if col.saturating_add(seg_w).saturating_add(ch_w) > max_width {
                if !seg.is_empty() {
                    self.write_live_fragment(seg.as_str());
                    col = col.saturating_add(seg_w);
                    seg.clear();
                    seg_w = 0;
                }
                if col >= max_width {
                    self.write_str("\n");
                    col = 0;
                }
            }

            seg.push(ch);
            seg_w = seg_w.saturating_add(ch_w);

            if col.saturating_add(seg_w) >= max_width {
                self.write_live_fragment(seg.as_str());
                col = col.saturating_add(seg_w);
                seg.clear();
                seg_w = 0;
                if col >= max_width {
                    self.write_str("\n");
                    col = 0;
                }
            }
        }

        if !seg.is_empty() {
            self.write_live_fragment(seg.as_str());
        }
    }

    fn flush_line(&self, s: &str) {
        // Normalize CRLF writers: `do_write` splits on `\n`, so lines may carry a trailing `\r`.
        // Keeping `\r` would cause cursor-return artifacts and break alignment.
        let s = s.trim_end_matches('\r');

        // Add to history
        self.history.borrow_mut().push(String::from(s));

        // If a live stream row is already inserted at row 3, only commit and repaint it.
        // Inserting again would duplicate the same line visually.
        if *self.live_line_inserted.borrow() {
            self.render_preview_line(s);
            *self.live_line_inserted.borrow_mut() = false;
            return;
        }

        let bottom = output_bottom_row(self.term_rows);

        self.inner.write_str(crate::ecma48::SAVE_CURSOR);
        self.inner.write_str(crate::ecma48::HIDE_CURSOR);

        // 1. Insert Line at Row 3
        self.inner
            .write_fmt(format_args!("{}", crate::ecma48::pos(3, 1)));
        self.inner.write_str("\x1b[L");

        // 2. Write content
        self.inner
            .write_fmt(format_args!("{}", crate::ecma48::pos(3, 2))); // Col 2 (skip scrollbar)

        // Clip/Pad content
        let content_len = crate::shell::ecma48::visible_width(s);
        let max_width = self.term_cols.saturating_sub(2);
        let padding = max_width.saturating_sub(content_len);

        for _ in 0..padding {
            self.inner.write_str(" ");
        }

        // We must manually clip s if it's too long, otherwise it wraps to next line col 1
        let final_s = if content_len > max_width {
            let mut clipped = String::new();
            let mut w = 0;
            for ch in s.chars() {
                w += 1;
                if w > max_width {
                    break;
                }
                clipped.push(ch);
            }
            clipped
        } else {
            String::from(s)
        };

        // Clock/System Message color: (120, 210, 255)
        self.inner.write_str("\x1b[38;2;120;210;255m");
        self.inner.write_str(&final_s);
        self.inner.write_str("\x1b[0m");

        // 3. Redraw ENTIRE scrollbar to fix the shift (offset 0 presumed)
        draw_scrollbar(
            self.inner,
            self.history.borrow().len(),
            bottom.saturating_sub(3),
            0,
            3,
            bottom,
        );

        self.inner.write_str(crate::ecma48::RESTORE_CURSOR);
        self.inner.write_str(crate::ecma48::SHOW_CURSOR);
    }

    fn echo_with_ansi(&self, text: &str, ansi: &str) {
        if text.is_empty() {
            return;
        }

        self.history.borrow_mut().push(String::from(text));
        let bottom = output_bottom_row(self.term_rows);

        self.inner
            .write_fmt(format_args!("{}", crate::ecma48::pos(3, 1)));
        self.inner.write_str("\x1b[L");

        self.inner
            .write_fmt(format_args!("{}", crate::ecma48::pos(3, 2)));
        self.inner.write_str(" ");
        self.inner.write_str(ansi);
        self.inner.write_str(text);
        self.inner.write_str("\x1b[0m");

        draw_scrollbar(
            self.inner,
            self.history.borrow().len(),
            bottom.saturating_sub(3),
            0,
            3,
            bottom,
        );
    }

    pub(crate) fn echo_command(&self, cmd: &str) {
        if cmd.is_empty() {
            return;
        }

        let verb = cmd.split_whitespace().next().unwrap_or("");
        let ansi = if verb.eq_ignore_ascii_case("install") || verb.eq_ignore_ascii_case("update") {
            "\x1b[38;2;255;55;255m"
        } else if verb.eq_ignore_ascii_case("ai") {
            "\x1b[38;2;150;150;150m"
        } else {
            "\x1b[37m"
        };
        self.echo_with_ansi(cmd, ansi);
    }

    pub(crate) fn echo_user_text(&self, text: &str) {
        self.echo_with_ansi(text, "\x1b[38;2;150;150;150m");
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

        self.inner
            .write_fmt(format_args!("{}", crate::ecma48::pos(3, 1)));
        self.inner.write_str("\x1b[L");

        // Repair
        self.inner
            .write_fmt(format_args!("{}", crate::ecma48::pos(4, 1)));
        self.inner.write_str("\x1b[38;2;80;80;80m│\x1b[0m");
        self.inner
            .write_fmt(format_args!("{}", crate::ecma48::pos(bottom, 1)));
        self.inner.write_str("\x1b[38;2;80;80;80m┴\x1b[0m");

        self.inner
            .write_fmt(format_args!("{}", crate::ecma48::pos(3, 1)));
        self.inner.write_str("\x1b[38;2;80;80;80m┬\x1b[0m ");
        self.inner.write_str(clipped.as_str());

        // Fix scrollbar below (draw_scrollbar handles < max height)
        // If history has 0 items, len=0. draw_scrollbar may do nothing if height is small?
        // Wait, write_overlay_hint doesn't push to history.
        // So history.len() is correct for what was there.
        // But write_overlay_hint overwrote row 3.
        // History items are stored. But visual row 3 is now hint.
        // It pushed down? "self.inner.write_str("\x1b[L");"
        // Yes it did insert line.
        // Does this mess up history view?
        // We inserted a visual line but didn't push to history.
        // Next scroll redraw will wipe it out. That's desired (hints are transient).
        // But for now we need scrollbar to look correct.

        draw_scrollbar(
            self.inner,
            self.history.borrow().len(),
            bottom.saturating_sub(3),
            0,
            3,
            bottom,
        );

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
        let mut buf = String::new();
        let _ = buf.write_fmt(args);
        self.do_write(buf.as_str());
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
    io.write_str(crate::ecma48::HIDE_CURSOR);
    // ccrate::ecma48::write_scroll_region(io, top, bottom);

    // Initial draw (total=0, offset=0)
    draw_scrollbar(io, 0, bottom.saturating_sub(top), 0, top, bottom);

    // Immediately force cursor to (3, 3)
    // crate::ecma48::write_pos(io, 3, 3);

    io.write_str(crate::ecma48::SHOW_CURSOR);
}

pub(crate) fn redraw_view(
    io: &dyn ShellIo,
    history: &[String],
    offset: usize,
    term_cols: usize,
    term_rows: usize,
) {
    let top = 3usize;
    let bottom = output_bottom_row(term_rows);
    let height = bottom.saturating_sub(top).saturating_add(1);
    let max_width = term_cols.saturating_sub(2);

    // Save cursor position to restore after drawing
    io.write_str(crate::ecma48::SAVE_CURSOR);
    io.write_str(crate::ecma48::HIDE_CURSOR);

    // Clear area by overwriting lines.
    // Row 3 displays history[len - 1 - offset]

    for i in 0..height {
        let row = top + i;
        io.write_fmt(format_args!("{}", crate::ecma48::pos(row, 2))); // Col 2 (skip scrollbar)

        let hist_idx_opt = history.len().checked_sub(1 + offset + i);

        // Clear To EOL
        io.write_str("\x1b[K");

        if let Some(hist_idx) = hist_idx_opt
            && let Some(line) = history.get(hist_idx)
        {
            // Determine color based on content?
            // For now, let's keep it simple.

            // Apply padding if needed (Right Align in Redraw too)
            let content_len = crate::shell::ecma48::visible_width(line);
            let padding = max_width.saturating_sub(content_len);
            for _ in 0..padding {
                io.write_str(" ");
            }

            let final_line = if content_len > max_width {
                let mut clipped = String::new();
                let mut w = 0;
                for ch in line.chars() {
                    w += 1;
                    if w > max_width {
                        break;
                    }
                    clipped.push(ch);
                }
                clipped
            } else {
                String::from(line)
            };

            io.write_str("\x1b[38;2;120;210;255m"); // Blue
            io.write_str(&final_line);
            io.write_str("\x1b[0m");
        }
    }

    draw_scrollbar(io, history.len(), height, offset, top, bottom);

    io.write_str(crate::ecma48::RESTORE_CURSOR);
    io.write_str(crate::ecma48::SHOW_CURSOR);
}

pub(crate) fn draw_scrollbar(
    io: &dyn ShellIo,
    total_lines: usize,
    viewport_height: usize,
    offset: usize,
    row_start: usize,
    row_end: usize,
) {
    // 1. Draw Top Joint
    io.write_fmt(format_args!("{}", crate::ecma48::pos(row_start, 1)));
    io.write_str("\x1b[38;2;80;80;80m┬\x1b[0m");

    // 2. Draw Bottom Joint
    io.write_fmt(format_args!("{}", crate::ecma48::pos(row_end, 1)));
    io.write_str("\x1b[38;2;80;80;80m┴\x1b[0m");

    let track_start = row_start + 1;
    let track_end = row_end - 1;
    if track_end < track_start {
        return;
    }

    let track_height = track_end - track_start + 1;

    // 3. Determine Indicator Position
    let thumb_row = if total_lines <= viewport_height {
        // Center
        row_start + (row_end - row_start) / 2
    } else {
        let max_offset = total_lines.saturating_sub(viewport_height);
        if max_offset == 0 {
            track_start
        } else {
            let eff_offset = core::cmp::min(offset, max_offset);
            // We want offset 0 (Newest) at Top (track_start)
            // max_offset (Oldest) at Bottom (track_end)
            track_start + (eff_offset * (track_height - 1)) / max_offset
        }
    };

    // 4. Draw Track + Thumb
    io.write_str("\x1b[38;2;80;80;80m"); // Dim Gray

    for r in track_start..=track_end {
        io.write_fmt(format_args!("{}", crate::ecma48::pos(r, 1)));
        if r == thumb_row {
            io.write_str("\x1b[38;2;150;150;150m%\x1b[38;2;80;80;80m");
        } else {
            io.write_str("│");
        }
    }

    io.write_str("\x1b[0m");
}
