use alloc::string::String;
use alloc::vec::Vec;

use super::super::{ShellBackend2, line_width_for_backend, print_shell_line};
use crate::disc::block::{self, DeviceHandle};

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

    pub(crate) fn autosize(
        headers: &'a [&'a str],
        preview_rows: &[&[&str]],
        max_width: usize,
    ) -> Self {
        let cols = headers.len().max(1);
        let mut max_cell_chars = headers
            .iter()
            .map(|cell| cell.chars().count())
            .max()
            .unwrap_or(MIN_COL_WIDTH)
            .max(MIN_COL_WIDTH);

        for row in preview_rows {
            for cell in row.iter().take(cols) {
                max_cell_chars = max_cell_chars.max(cell.chars().count());
            }
        }

        let wanted_width = FRAME_SIDE_WIDTH
            + cols.saturating_mul(max_cell_chars)
            + cols.saturating_sub(1).saturating_mul(CELL_SEPARATOR.len());

        Self::with_width(headers, wanted_width.min(max_width))
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

pub(crate) fn print_table<const N: usize>(
    io: &'static dyn ShellBackend2,
    headers: &[&str; N],
    rows: &[[&str; N]],
) {
    let table = TlbTable::with_width(headers, line_width_for_backend(io).saturating_sub(2));
    table.emit_header(|text| print_shell_line(io, text));
    for row in rows {
        table.emit_row(row, |text| print_shell_line(io, text));
    }
    table.emit_footer(|text| print_shell_line(io, text));
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
        let (status, err) = crate::wait::spawn_and_wait_local(async move {
            crate::r::disc::detect::detect_physical_disk_detail(handle).await
        });
        out.push(DiskChoice {
            handle,
            status,
            err,
        });
    }

    out.sort_by_key(|c| c.raw_id());
    out
}

pub(crate) fn print_disk_choice_table(
    io: &'static dyn ShellBackend2,
    prefix: &str,
    title: &str,
    choices: &[DiskChoice],
) {
    print_shell_line(io, alloc::format!("{prefix}: {title}").as_str());

    if choices.is_empty() {
        print_shell_line(
            io,
            alloc::format!("{prefix}: no top-level disk devices found").as_str(),
        );
        return;
    }

    let headers = ["ID", "Name", "Size", "Mode", "Status", "Label"];
    let table = TlbTable::with_width(&headers, line_width_for_backend(io).saturating_sub(2));
    table.emit_header(|text| print_shell_line(io, text));
    for choice in choices {
        let raw = alloc::format!("{}", choice.raw_id());
        let name = alloc::format!("{}", choice.handle.id());
        let size = choice.size_text();
        let mode = choice.mode_text();
        let status = choice.status_text();
        let label = choice.label_text();
        let row = [
            raw.as_str(),
            name.as_str(),
            size.as_str(),
            mode,
            status.as_str(),
            label.as_str(),
        ];
        table.emit_row(&row, |text| print_shell_line(io, text));
    }
    table.emit_footer(|text| print_shell_line(io, text));
}

fn push_cell(out: &mut String, text: &str, width: usize) {
    if width == 0 {
        return;
    }

    let text_chars = text.chars().count();
    if text_chars <= width {
        out.push_str(text);
        for _ in 0..width - text_chars {
            out.push(' ');
        }
        return;
    }

    if width <= 3 {
        for ch in text.chars().take(width) {
            out.push(ch);
        }
        return;
    }

    for ch in text.chars().take(width - 3) {
        out.push(ch);
    }
    out.push_str("...");
}
