//! ANSI styling, incremental screen updates, and native cursor placement.
use super::super::{tui, update};
use super::{MetaFmtStr, RgbaColor, SpecialRows, Terminal};
use alloc::{format, string::String, vec::Vec};
use trueos_terminal::{MouseEncoding, MouseOptions, MouseTracking};

/// Client-visible frame baseline and cursor, including the TUI handoff state.
pub(super) struct AnsiView {
    pub(super) lines: Option<Vec<update::RenderedLine>>,
    pub(super) cursor_column: Option<usize>,
    pub(super) terminal_active: bool,
    mouse: MouseOptions,
}

impl AnsiView {
    pub(super) fn new(enabled: bool) -> Self {
        Self {
            lines: enabled.then(Vec::new),
            cursor_column: None,
            terminal_active: false,
            mouse: MouseOptions::default(),
        }
    }
}

impl Terminal {
    pub(super) fn clear_screen(&mut self) {
        self.write(b"\x1b[0m\x1b[2J\x1b[H");
    }

    fn apply_mouse_options(&mut self, options: MouseOptions) {
        // Encoding flags without tracking must not capture the host's mouse.
        let options = if options.tracking == MouseTracking::Off {
            MouseOptions::default()
        } else {
            options
        };
        if self.view.mouse == options {
            return;
        }
        self.write(b"\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l\x1b[?1015l");
        match options.encoding {
            MouseEncoding::Sgr => self.write(b"\x1b[?1006h"),
            MouseEncoding::Urxvt => self.write(b"\x1b[?1015h"),
            MouseEncoding::Legacy => {}
        }
        match options.tracking {
            MouseTracking::Buttons => self.write(b"\x1b[?1000h"),
            MouseTracking::Drag => self.write(b"\x1b[?1002h"),
            MouseTracking::Any => self.write(b"\x1b[?1003h"),
            MouseTracking::Off => {}
        }
        self.view.mouse = options;
    }

    pub(super) fn write_meta(&mut self, run: &MetaFmtStr) {
        if run.color.is_none() && !run.underline && !run.blink {
            self.write(run.text.as_bytes());
            return;
        }
        self.write(b"\x1b[0m");
        if let Some(color) = run.color {
            let [r, g, b, _] = color.rgba();
            self.write(format!("\x1b[38;2;{r};{g};{b}m").as_bytes());
            if let Some([r, g, b, _]) = color.background()
                .filter(|bg| *bg != RgbaColor::BlackTransparent.rgba())
            {
                self.write(format!("\x1b[48;2;{r};{g};{b}m").as_bytes());
            }
        }
        if run.underline || run.color.is_some_and(RgbaColor::underline) {
            self.write(b"\x1b[4m");
        }
        if run.blink || run.color.is_some_and(RgbaColor::blink) {
            self.write(b"\x1b[5m");
        }
        self.write(run.text.as_bytes());
        self.write(b"\x1b[0m");
    }

    // One-based screen rows: the first three are controls until a TUI takes over.
    pub(super) fn set_matrix_region(&mut self) {
        let (_, rows) = self.shell.get_size();
        if self.view.terminal_active {
            self.write(b"\x1b[r");
        } else {
            self.write(format!("\x1b[4;{rows}r").as_bytes());
        }
    }

    pub(super) fn refresh_controls(&mut self) {
        if self.view.lines.is_none() || self.closing {
            return;
        }
        if let Some(drain) = tui::take_remote_output(
            self.shell.tui_frontend, super::OUTPUT_LIMIT.saturating_sub(self.output.len()),
        ) {
            self.write(&drain.bytes);
            if drain.failed && !drain.pending {
                self.overflow = true;
                self.closing = true;
                return;
            }
            if drain.active || drain.pending {
                self.view.terminal_active = true;
                return;
            }
            if drain.repaint {
                self.view.terminal_active = false;
                self.view.mouse = MouseOptions::default();
                self.view.cursor_column = None;
                self.view.lines.as_mut().unwrap().clear();
                self.set_matrix_region();
            }
        }
        let app =
            tui::snapshot(self.shell.tui_frontend, self.shell.active_matrix_slot_name().as_deref());
        let active = app.is_some();
        let mouse = if active {
            tui::mouse_options(
                self.shell.tui_frontend,
                self.shell.active_matrix_slot_name().as_deref(),
            )
        } else {
            MouseOptions::default()
        };
        self.apply_mouse_options(mouse);
        if active != self.view.terminal_active {
            self.view.terminal_active = active;
            self.clear_screen();
            self.set_matrix_region();
            self.view.lines.as_mut().unwrap().clear();
        }
        let mut current =
            app.unwrap_or_else(|| self.shell.capture_matrix_snapshot().rendered_lines());
        // Pixel renderers need a concrete base color. ANSI matrix cells use
        // the client terminal’s default instead; retain explicit highlights.
        if !active {
            for line in current.iter_mut().skip(3) {
                for (_, style) in line {
                    if let Some(color) = *style
                        && color.background() == Some(update::MATRIX_BACKGROUND)
                    {
                        *style = Some(if color.blink() {
                            RgbaColor::Blinking {
                                foreground: color.rgba(), background: None, underline: color.underline(),
                            }
                        } else {
                            RgbaColor::Terminal {
                                foreground: color.rgba(),
                                background: RgbaColor::BlackTransparent.rgba(),
                                underline: color.underline(),
                            }
                        });
                    }
                }
            }
        }
        let previous = self.view.lines.as_ref().unwrap();
        // The native cursor supplies the blinking block; its underlying cell
        // remains a plain blank, carrying no printable cursor glyph.
        let cursor = if active {
            None
        } else {
            current.get(2).and_then(|row| {
                row.iter()
                    .position(|(_, color)| color.is_some_and(RgbaColor::blink))
            })
        };
        if let Some(column) = cursor {
            current[2][column].1 = Some(update::cell_color(None, update::CONTROL_BACKGROUND));
        }
        let mut previous = previous.clone();
        let mut shifted = false;
        if !active
            && previous.len() == current.len()
            && previous.len() > 4
            && previous[3..] != current[3..]
            && previous[3].len() == current[3].len()
        {
            let height = current.len() - 3;
            for count in 1..height {
                let up = previous[3 + count..] == current[3..current.len() - count];
                let down = previous[3..previous.len() - count] == current[3 + count..];
                if up || down {
                    shifted = true;
                    self.write(
                        format!("\x1b[0m\x1b[4;1H\x1b[{count}{}", if up { 'S' } else { 'T' })
                            .as_bytes(),
                    );
                    let matrix = &mut previous[3..];
                    if up {
                        matrix.rotate_left(count);
                    } else {
                        matrix.rotate_right(count);
                    }
                    let exposed = if up { height - count..height } else { 0..count };
                    for row in exposed {
                        matrix[row] = alloc::vec![(' ', None); current[3].len()];
                    }
                    break;
                }
            }
        }
        let updates = update::diff_rendered_lines(
            if previous.is_empty() {
                None
            } else {
                Some(&previous)
            },
            &current,
        );
        let changed = shifted || !updates.is_empty();
        for update in updates {
            let row = match update.row {
                SpecialRows::TitleRow => 1,
                SpecialRows::StatusRow => 2,
                SpecialRows::PromtRow => 3,
                SpecialRows::MatrixRow(index) => index + 4,
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
                self.write(
                    " ".repeat(update.remove - update.text.chars().count())
                        .as_bytes(),
                );
            }
        }
        if cursor.is_some() != self.view.cursor_column.is_some() {
            self.write(if cursor.is_some() {
                b"\x1b[?25h"
            } else {
                b"\x1b[?25l"
            });
        }
        if let Some(column) = cursor
            && (changed || self.view.cursor_column != cursor)
        {
            self.write(format!("\x1b[3;{}H", column + 1).as_bytes());
        }
        self.view.cursor_column = cursor;
        self.view.lines = Some(current);
    }

    pub(super) fn prompt(&mut self) {
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

    pub(super) fn reset_input(&mut self) {
        if self.view.lines.is_some() {
            return;
        }
        self.write(b"\x1b8\x1b[K");
        if self.prompt_name != self.shell.active_matrix_slot_name().unwrap_or_default() {
            self.write(b"\r\x1b[2K");
            self.prompt();
        }
    }
}
