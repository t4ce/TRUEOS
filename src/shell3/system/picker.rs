//! Shared selection and literal text entry for native system tools.
use super::helper::{Action, Input};
use alloc::{format, string::String, vec::Vec};
pub(super) enum Event {
    Choose(usize),
    Submit(String),
    Quit,
}
#[derive(Default)]
pub(super) struct Picker {
    pub(super) selected: usize,
    input: Input,
    form: Option<String>,
    value: String,
    after_cr: bool,
}
impl Picker {
    pub(super) fn form(&mut self, prompt: &str) {
        self.form = Some(prompt.into());
        self.value.clear();
        self.input = Input::default();
    }
    pub(super) fn clear_form(&mut self) {
        self.form = None;
        self.value.clear();
        self.input = Input::default();
    }
    fn layout(&self, rows: usize, count: usize) -> (usize, usize, usize) {
        let first = if rows >= 10 { 4 } else { 3 };
        let visible = rows.saturating_sub(first).min(count);
        let offset = self.selected.saturating_sub(visible.saturating_sub(1));
        (first, visible, offset)
    }
    pub(super) fn input(
        &mut self,
        bytes: &[u8],
        count: usize,
        rows: usize,
        now: u64,
    ) -> Option<Event> {
        if self.form.is_some() {
            for &byte in bytes {
                if byte == 27 || self.input.in_sequence() {
                    for action in self.input.feed(&[byte], now) {
                        if matches!(action, Action::Quit | Action::Click(1)) {
                            return Some(Event::Quit);
                        }
                    }
                    continue;
                }
                if byte == b'\n' && self.after_cr {
                    self.after_cr = false;
                    continue;
                }
                self.after_cr = byte == b'\r';
                match byte {
                    b'\r' | b'\n' => return Some(Event::Submit(core::mem::take(&mut self.value))),
                    3 => return Some(Event::Quit),
                    8 | 127 => {
                        self.value.pop();
                    }
                    32..=126 if self.value.len() < 256 => self.value.push(byte as char),
                    _ => {}
                }
            }
            return (self.input.timeout(now) == Some(Action::Quit)).then_some(Event::Quit);
        }
        let mut actions = self.input.feed(bytes, now);
        if let Some(action) = self.input.timeout(now) {
            actions.push(action);
        }
        for action in actions {
            match action {
                Action::Up if count > 0 => self.selected = (self.selected + count - 1) % count,
                Action::Down if count > 0 => self.selected = (self.selected + 1) % count,
                Action::Choose if count > 0 => return Some(Event::Choose(self.selected)),
                Action::Quit | Action::Click(1) => return Some(Event::Quit),
                Action::Click(row) => {
                    let (first, visible, offset) = self.layout(rows, count);
                    if (first..first + visible).contains(&row) {
                        self.selected = offset + row - first;
                        return Some(Event::Choose(self.selected));
                    }
                }
                _ => {}
            }
        }
        None
    }
    pub(super) fn frame(
        &self,
        title: &str,
        summary: &str,
        labels: &[&str],
        rows: usize,
    ) -> Vec<String> {
        let mut lines = alloc::vec![String::new();rows];
        if rows < 5 {
            return lines;
        }
        lines[0] = title.into();
        lines[1] = if self.form.is_some() {
            "Enter submit   Esc Return".into()
        } else {
            "↑/↓ j/k select   Enter choose   Esc/q Return   Mouse choose".into()
        };
        lines[2] = summary.into();
        if let Some(prompt) = &self.form {
            lines[3] = prompt.clone();
            lines[4] = format!("> {}", self.value);
        } else {
            let (first, visible, offset) = self.layout(rows, labels.len());
            for index in 0..visible {
                lines[first + index] = format!(
                    "  {} {}",
                    if offset + index == self.selected {
                        "›"
                    } else {
                        " "
                    },
                    labels[offset + index]
                );
            }
        }
        lines
    }
}
pub(super) fn wrap(lines: Vec<String>, cols: usize) -> Vec<String> {
    let width = cols.max(1);
    let mut output = Vec::new();
    for line in lines {
        let chars: Vec<char> = line.chars().collect();
        if chars.is_empty() {
            output.push(String::new());
        } else {
            for chunk in chars.chunks(width) {
                output.push(chunk.iter().collect());
            }
        }
    }
    output
}
pub(super) fn page(
    title: &str,
    summary: &str,
    content: &[String],
    scroll: &mut usize,
    rows: usize,
) -> Vec<String> {
    let mut lines = alloc::vec![String::new();rows];
    if rows < 5 {
        return lines;
    }
    lines[0] = title.into();
    lines[1] = "↑/↓ j/k scroll   Enter/Esc Return   Mouse wheel scroll".into();
    lines[2] = summary.into();
    let visible = rows - 3;
    *scroll = (*scroll).min(content.len().saturating_sub(visible));
    for (index, line) in content.iter().skip(*scroll).take(visible).enumerate() {
        lines[3 + index] = line.clone();
    }
    lines
}
