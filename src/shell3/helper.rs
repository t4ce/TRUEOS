//! Common admission, controls and changed-cell painting for host terminal UIs.
use super::tui::{self, Frontend};
use crate::shell3::{MatrixTarget};
use crate::shell2::{self};
use alloc::{format, string::String, vec::Vec};

const GO: [char; 9] = ['⣿', '⣾', '⣽', '⣻', '⢿', '⡿', '⣟', '⣯', '⣷'];
pub(super) fn spinner(now: u64) -> char {
    GO[(now / 100_000_000 % GO.len() as u64) as usize]
}

/// Count finite work, rather than the lifetime or selection of its menu.
pub(super) struct Work(MatrixTarget);
impl Work {
    pub(super) fn new(target: &MatrixTarget) -> Self {
        shell2::set_matrix_target_active(target, true);
        Self(target.clone())
    }
}
impl Drop for Work {
    fn drop(&mut self) {
        shell2::set_matrix_target_active(&self.0, false);
    }
}

/// A parked helper is reused; only a new lease needs a worker task.
pub(super) fn admit(
    name: &str,
    frontend: Frontend,
) -> Result<Option<(MatrixTarget, crate::workers::WorkerSpawner)>, String> {
    if tui::native_slot(name) {
        tui::request(frontend, name).map_err(String::from)?;
        return Ok(None);
    }
    let spawner =
        crate::workers::pick_background_spawner().ok_or("No background worker is ready.")?;
    let origin = shell2::matrix_target_for_slot_name(shell2::OUTPUT_SYSTEM_MASK, "");
    let target = shell2::claim_matrix_target_for_named_app_slot(&origin, name, name)
        .ok_or("Helper slot is occupied.")?;
    tui::attach_native(frontend, &target)?;
    Ok(Some((target, spawner)))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Action {
    Up,
    Down,
    Choose,
    Quit,
    Click(usize),
}
#[derive(Default)]
pub(super) struct Input {
    sequence: Vec<u8>,
    escape_at: u64,
    after_cr: bool,
}
impl Input {
    pub(super) fn in_sequence(&self) -> bool {
        !self.sequence.is_empty()
    }
    pub(super) fn feed(&mut self, bytes: &[u8], now: u64) -> Vec<Action> {
        self.feed_cells(bytes, now)
            .into_iter()
            .map(|(action, _)| action)
            .collect()
    }
    /// Column coordinates let a row expose a separate action tag.
    pub(super) fn feed_cells(&mut self, bytes: &[u8], now: u64) -> Vec<(Action, Option<usize>)> {
        let mut actions = Vec::new();
        for &byte in bytes {
            if !self.sequence.is_empty() {
                if self.sequence.len() == 1 && !matches!(byte, b'[' | b'O') {
                    self.sequence.clear();
                    actions.push((Action::Quit, None));
                    continue;
                }
                self.sequence.push(byte);
                if self.sequence.len() > 2 && (0x40..=0x7e).contains(&byte) {
                    if let Some(action) = decode_cell_sequence(&self.sequence) {
                        actions.push(action);
                    }
                    self.sequence.clear();
                } else if self.sequence.len() > 64 {
                    self.sequence.clear();
                }
                continue;
            }
            if byte == b'\n' && self.after_cr {
                self.after_cr = false;
                continue;
            }
            self.after_cr = byte == b'\r';
            let action = match byte {
                27 => {
                    self.sequence.push(byte);
                    self.escape_at = now;
                    None
                }
                b'k' => Some(Action::Up),
                b'j' => Some(Action::Down),
                b'\r' | b'\n' => Some(Action::Choose),
                b'q' | b'h' | 3 => Some(Action::Quit),
                _ => None,
            };
            if let Some(action) = action {
                actions.push((action, None));
            }
        }
        actions
    }
    pub(super) fn timeout(&mut self, now: u64) -> Option<Action> {
        if !self.sequence.is_empty() && now.saturating_sub(self.escape_at) >= 75_000_000 {
            let lone_escape = self.sequence.len() == 1;
            self.sequence.clear();
            return lone_escape.then_some(Action::Quit);
        }
        None
    }
}
fn decode_cell_sequence(bytes: &[u8]) -> Option<(Action, Option<usize>)> {
    match bytes {
        b"\x1b[A" | b"\x1bOA" => Some((Action::Up, None)),
        b"\x1b[B" | b"\x1bOB" => Some((Action::Down, None)),
        b"\x1b[D" | b"\x1bOD" => Some((Action::Quit, None)),
        _ => {
            let report = bytes.strip_prefix(b"\x1b[<")?;
            if report.last() != Some(&b'M') {
                return None;
            } // no release/double trigger
            let text = core::str::from_utf8(&report[..report.len() - 1]).ok()?;
            let mut fields = text.split(';');
            let button = fields.next()?.parse::<u16>().ok()?;
            let col = fields.next()?.parse::<usize>().ok()?.checked_sub(1)?;
            let row = fields.next()?.parse::<usize>().ok()?.checked_sub(1)?;
            if fields.next().is_some() {
                return None;
            }
            match button & !28 {
                0 => Some((Action::Click(row), Some(col))),
                64 => Some((Action::Up, None)),
                65 => Some((Action::Down, None)),
                _ => None,
            }
        }
    }
}

#[derive(Default)]
pub(super) struct Screen {
    previous: Vec<Vec<char>>,
    geometry: (usize, usize),
}
impl Screen {
    pub(super) fn invalidate(&mut self) {
        for row in &mut self.previous {
            row.fill(' ');
        }
        self.geometry = (0, 0);
    }
    /// Blank padding erases shrinking text. ANSI positions use glyph columns,
    /// so UTF-8 bars and arrows never turn byte offsets into cursor positions.
    pub(super) fn diff(&mut self, lines: &[String], cols: usize, rows: usize) -> String {
        let resized = self.geometry != (cols, rows);
        let mut output = String::new();
        if resized {
            output.push_str("\x1b[?25l\x1b[?1000h\x1b[?1006h\x1b[0m\x1b[2J");
            self.previous = alloc::vec![alloc::vec![' '; cols]; rows];
            self.geometry = (cols, rows);
        }
        for row in 0..rows {
            let mut current = alloc::vec![' '; cols];
            if let Some(line) = lines.get(row) {
                for (col, ch) in line.chars().take(cols).enumerate() {
                    current[col] = if ch.is_control() { ' ' } else { ch };
                }
            }
            let previous = &self.previous[row];
            let mut col = 0;
            while col < cols {
                if current[col] == previous[col] {
                    col += 1;
                    continue;
                }
                let start = col;
                while col < cols && current[col] != previous[col] {
                    col += 1;
                }
                output.push_str(&format!("\x1b[{};{}H", row + 1, start + 1));
                output.extend(current[start..col].iter().copied());
            }
            self.previous[row] = current;
        }
        output
    }
    pub(super) fn paint(
        &mut self,
        target: &MatrixTarget,
        lines: &[String],
        cols: usize,
        rows: usize,
    ) {
        let bytes = self.diff(lines, cols, rows);
        if !bytes.is_empty() {
            tui::native_write(target, bytes.as_bytes());
        }
    }
}
