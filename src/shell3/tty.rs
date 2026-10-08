//! Shell3 terminal session: lifecycle, transport output, and view resizing.
mod ansi;
mod input;

use super::{MetaFmtStr, RgbaColor, Shell3, SpecialRows};
use alloc::{string::String, vec::Vec};
use ansi::AnsiView;
use input::{CommandHistory, InputDecoder};

const LINE_LIMIT: usize = 1024;
// Bounded queue large enough for the maximum accepted SSH frame (512 × 256).
pub(super) const OUTPUT_LIMIT: usize = 1024 * 1024;

pub(super) struct Terminal {
    shell: Shell3,
    line: String,
    line_overflow: bool,
    history: CommandHistory,
    decoder: InputDecoder,
    prompt_name: String,
    view: AnsiView,
    pub output: Vec<u8>,
    pub closing: bool,
    pub overflow: bool,
}

impl Terminal {
    pub fn new(shell: Shell3) -> Self {
        Self::start(shell, false)
    }

    pub fn new_ssh(shell: Shell3) -> Self {
        Self::start(shell, true)
    }

    fn start(mut shell: Shell3, controls: bool) -> Self {
        shell.set_mode(3);
        let mut terminal = Self {
            shell,
            line: String::new(),
            line_overflow: false,
            history: CommandHistory::default(),
            decoder: InputDecoder::default(),
            prompt_name: String::new(),
            view: AnsiView::new(controls),
            output: Vec::new(),
            closing: false,
            overflow: false,
        };
        // Save the local wheel mode and stop alternate-screen wheel events
        // from becoming arrow keys in nc's locally echoed input buffer.
        terminal.write(b"\x1b[?1007s\x1b[?1007l\x1b[?1049h\x1b[0m\x1b[2J\x1b[H");
        if controls {
            // Leave mouse handling to the client terminal (selection and menus).
            terminal.write(b"\x1b[1 q\x1b[?25h");
            terminal.set_matrix_region();
            terminal.refresh_controls();
            return terminal;
        }
        let title = terminal.shell.row_for_render(SpecialRows::TitleRow);
        for run in &title.left {
            terminal.write_meta(run);
        }
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

    fn notice(&mut self, text: &str) {
        if self.view.lines.is_some() {
            self.shell.record_terminal_notice(text);
        } else {
            self.write(b"\r\n");
            self.write(text.as_bytes());
            self.write(b"\r\n");
        }
    }

    pub(super) fn reconcile_matrix_selection(&mut self) {
        self.shell.reconcile_matrix_selection();
        self.refresh_controls();
    }

    pub(super) fn resize(&mut self, columns: usize, rows: usize) {
        if self.shell.get_size() != (columns, rows) {
            self.shell.set(columns, rows);
            if let Some(lines) = self.view.lines.as_mut() {
                lines.clear();
                self.write(b"\x1b[0m\x1b[2J\x1b[H");
            }
            if self.view.lines.is_some() {
                self.set_matrix_region();
            }
            self.refresh_controls();
        }
    }
}
