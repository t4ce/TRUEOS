use alloc::string::String;
use alloc::vec::Vec;

use crate::disc::block::{self, DeviceHandle};
use unicode_width::UnicodeWidthStr;

const CELL_SEPARATOR: &str = " | ";
const FRAME_SIDE_WIDTH: usize = 4;
const MIN_COL_WIDTH: usize = 3;

#[derive(Clone)]
pub(crate) struct DiskChoice {
    pub(crate) handle: DeviceHandle,
    pub(crate) status: crate::r::disc::detect::DiscStatus,
    pub(crate) err: Option<block::Error>,
}

impl DiskChoice {
    pub(crate) fn raw_id(&self) -> u32 {
        self.handle.id().raw()
    }

    pub(crate) fn size_text(&self) -> String {
        let info = self.handle.info();
        let total = info.block_count.saturating_mul(info.block_size as u64);
        if total >= 1024 * 1024 * 1024 {
            alloc::format!("{}GB", total / (1024 * 1024 * 1024))
        } else if total >= 1024 * 1024 {
            alloc::format!("{}MB", total / (1024 * 1024))
        } else {
            alloc::format!("{}KB", total / 1024)
        }
    }

    pub(crate) fn mode_text(&self) -> &'static str {
        if self.handle.info().writable {
            "rw"
        } else {
            "ro"
        }
    }

    pub(crate) fn label_text(&self) -> String {
        String::from(self.handle.info().label.as_deref().unwrap_or("-"))
    }

    pub(crate) fn status_text(&self) -> String {
        match (&self.status, self.err) {
            (crate::r::disc::detect::DiscStatus::Unknown, None) => String::from("registered"),
            (crate::r::disc::detect::DiscStatus::Unknown, Some(err)) => {
                alloc::format!("{}:{:?}", self.status.short(), err)
            }
            _ => String::from(self.status.short()),
        }
    }
}

pub(crate) struct TlbTable<'a> {
    headers: &'a [&'a str],
    width: usize,
    col_widths: Vec<usize>,
}

impl<'a> TlbTable<'a> {
    pub(crate) fn with_width(headers: &'a [&'a str], width: usize) -> Self {
        let cols = headers.len().max(1);
        let min_content = cols.saturating_mul(MIN_COL_WIDTH)
            + cols.saturating_sub(1).saturating_mul(CELL_SEPARATOR.len());
        let width = width.max(min_content + FRAME_SIDE_WIDTH);
        let content_width = width.saturating_sub(FRAME_SIDE_WIDTH);
        let separator_width = cols.saturating_sub(1).saturating_mul(CELL_SEPARATOR.len());
        let cell_budget = content_width.saturating_sub(separator_width);
        let base = cell_budget / cols;
        let extra = cell_budget % cols;
        let mut col_widths = Vec::with_capacity(cols);
        for idx in 0..cols {
            col_widths.push(base + usize::from(idx < extra));
        }

        Self {
            headers,
            width,
            col_widths,
        }
    }

    pub(crate) fn with_max_col_widths(mut self, max_widths: &[usize]) -> Self {
        if self.col_widths.is_empty() || max_widths.is_empty() {
            return self;
        }

        loop {
            let mut freed = 0usize;
            for (idx, width) in self.col_widths.iter_mut().enumerate() {
                let cap = max_widths.get(idx).copied().unwrap_or(0);
                if cap == 0 || *width <= cap {
                    continue;
                }
                freed = freed.saturating_add(*width - cap);
                *width = cap;
            }

            if freed == 0 {
                break;
            }

            let eligible: Vec<usize> = self
                .col_widths
                .iter()
                .enumerate()
                .filter_map(|(idx, width)| {
                    let cap = max_widths.get(idx).copied().unwrap_or(0);
                    if cap == 0 || *width < cap {
                        Some(idx)
                    } else {
                        None
                    }
                })
                .collect();

            if eligible.is_empty() {
                if let Some(last) = self.col_widths.last_mut() {
                    *last = last.saturating_add(freed);
                }
                break;
            }

            let base = freed / eligible.len();
            let extra = freed % eligible.len();
            for (pos, idx) in eligible.iter().copied().enumerate() {
                self.col_widths[idx] =
                    self.col_widths[idx].saturating_add(base + usize::from(pos < extra));
            }
        }

        self
    }

    pub(crate) fn emit_header<F>(&self, mut emit_line: F)
    where
        F: FnMut(&str),
    {
        emit_line(self.rule_line('└', '┘').as_str());
        emit_line(self.cells_line(self.headers.iter().copied()).as_str());
        emit_line(self.rule_line('├', '┤').as_str());
    }

    pub(crate) fn emit_row<S: AsRef<str>, F>(&self, cells: &[S], mut emit_line: F)
    where
        F: FnMut(&str),
    {
        emit_line(self.cells_line(cells.iter().map(AsRef::as_ref)).as_str());
    }

    pub(crate) fn emit_footer<F>(&self, mut emit_line: F)
    where
        F: FnMut(&str),
    {
        emit_line(self.rule_line('┌', '┐').as_str());
    }

    fn rule_line(&self, left: char, right: char) -> String {
        let mut line = String::with_capacity(self.width + 2);
        line.push(left);
        for _ in 0..self.width.saturating_sub(2) {
            line.push('─');
        }
        line.push(right);
        line
    }

    fn cells_line<'b>(&self, cells: impl Iterator<Item = &'b str>) -> String {
        let mut line = String::with_capacity(self.width + 2);
        line.push('│');
        line.push(' ');

        let mut cells = cells;
        for (idx, col_width) in self.col_widths.iter().copied().enumerate() {
            if idx > 0 {
                line.push_str(CELL_SEPARATOR);
            }

            let cell = cells.next().unwrap_or("");
            push_cell(&mut line, cell, col_width);
        }

        line.push(' ');
        line.push('│');
        line
    }
}

pub(crate) fn parse_disc_id_raw(s: &str) -> Option<u32> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let s = s.strip_prefix("disc").unwrap_or(s);
    s.parse::<u32>().ok()
}

#[inline]
fn is_user_visible_top_level(handle: &DeviceHandle) -> bool {
    let info = handle.info();
    info.parent.is_none() && info.user_visible
}

pub(crate) fn select_top_level_disk(raw_id: u32) -> Option<DeviceHandle> {
    block::device_handles()
        .into_iter()
        .find(|handle| is_user_visible_top_level(handle) && handle.id().raw() == raw_id)
}

pub(crate) fn collect_top_level_disk_choices() -> Vec<DiskChoice> {
    use alloc::collections::BTreeSet;

    let mut ids = BTreeSet::new();
    let mut handles = Vec::new();
    for handle in block::device_handles().into_iter() {
        if is_user_visible_top_level(&handle) && ids.insert(handle.id().raw()) {
            handles.push(handle);
        }
    }

    // Also include mounted TRUEOSFS roots so interactive admin flows keep seeing
    // a just-created/mounted disk even if its visibility metadata differs.
    for root in crate::r::fs::trueosfs::list_roots() {
        if let Some(handle) = block::device_handle(root.disk_id)
            && handle.info().parent.is_none()
            && ids.insert(handle.id().raw())
        {
            handles.push(handle);
        }
    }

    let mut out = Vec::new();
    for handle in handles.into_iter() {
        out.push(DiskChoice {
            handle,
            status: crate::r::disc::detect::DiscStatus::Unknown,
            err: None,
        });
    }

    out.sort_by_key(|c| c.raw_id());
    out
}

fn push_cell(out: &mut String, text: &str, width: usize) {
    if width == 0 {
        return;
    }

    let text_width = UnicodeWidthStr::width(text);
    if text_width <= width {
        out.push_str(text);
        for _ in 0..width - text_width {
            out.push(' ');
        }
        return;
    }

    if width <= 3 {
        push_visible_prefix(out, text, width);
        return;
    }

    push_visible_prefix(out, text, width - 3);
    out.push_str("...");
}

fn push_visible_prefix(out: &mut String, text: &str, width: usize) {
    let bytes = text.as_bytes();
    let mut index = 0usize;
    let mut used = 0usize;
    let mut accepting_text = true;

    while index < bytes.len() {
        if bytes[index] == 0x1b {
            let end = control_sequence_end(text, index);
            out.push_str(&text[index..end]);
            index = end;
            continue;
        }

        let Some(ch) = text[index..].chars().next() else {
            break;
        };
        let end = index + ch.len_utf8();
        if accepting_text {
            let char_width = UnicodeWidthStr::width(&text[index..end]);
            if used.saturating_add(char_width) <= width {
                out.push(ch);
                used = used.saturating_add(char_width);
            } else {
                accepting_text = false;
            }
        }
        index = end;
    }
}

fn control_sequence_end(text: &str, start: usize) -> usize {
    let bytes = text.as_bytes();
    if start + 1 >= bytes.len() {
        return bytes.len();
    }

    match bytes[start + 1] {
        b'[' => {
            let mut index = start + 2;
            while index < bytes.len() {
                let byte = bytes[index];
                index += 1;
                if (0x40..=0x7e).contains(&byte) {
                    break;
                }
            }
            index
        }
        b']' => {
            let mut index = start + 2;
            while index < bytes.len() {
                if bytes[index] == 0x07 {
                    return index + 1;
                }
                if bytes[index] == 0x1b && index + 1 < bytes.len() && bytes[index + 1] == b'\\' {
                    return index + 2;
                }
                index += 1;
            }
            index
        }
        _ => {
            let scalar_len = text[start + 1..]
                .chars()
                .next()
                .map(char::len_utf8)
                .unwrap_or(0);
            start + 1 + scalar_len
        }
    }
}
