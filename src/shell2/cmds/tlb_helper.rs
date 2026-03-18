use alloc::string::String;
use alloc::vec::Vec;

use crate::shell2::ShellIo2;

const CELL_SEPARATOR: &str = " | ";
const FRAME_SIDE_WIDTH: usize = 4;
const MIN_COL_WIDTH: usize = 3;

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

    pub(crate) fn print_header(&self, io: &dyn ShellIo2) {
        self.print_rule(io, '┌', '┐');
        self.print_cells(io, self.headers.iter().copied());
        self.print_rule(io, '├', '┤');
    }

    pub(crate) fn print_row<S: AsRef<str>>(&self, io: &dyn ShellIo2, cells: &[S]) {
        self.print_cells(io, cells.iter().map(AsRef::as_ref));
    }

    pub(crate) fn print_footer(&self, io: &dyn ShellIo2) {
        self.print_rule(io, '└', '┘');
    }

    fn print_rule(&self, io: &dyn ShellIo2, left: char, right: char) {
        let mut line = String::with_capacity(self.width + 2);
        line.push(left);
        for _ in 0..self.width.saturating_sub(2) {
            line.push('─');
        }
        line.push(right);
        line.push_str("\r\n");
        io.write_str(line.as_str());
    }

    fn print_cells<'b>(&self, io: &dyn ShellIo2, cells: impl Iterator<Item = &'b str>) {
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
        line.push_str("\r\n");
        io.write_str(line.as_str());
    }
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
