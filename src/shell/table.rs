use alloc::string::String;
use core::fmt::Write;
use crate::shell::ShellIo;

pub struct TableColumn {
    pub header: &'static str,
    pub width: usize,
}

pub struct Table<'a> {
    cols: &'a [TableColumn],
}

impl<'a> Table<'a> {
    pub fn new(cols: &'a [TableColumn]) -> Self {
        Self { cols }
    }

    pub fn print_header(&self, io: &dyn ShellIo) {
        let mut line: String = String::new();
        
        // Top border
        // self.print_separator(io);

        // Header row
        for col in self.cols {
            let _ = write!(line, "{:width$}  ", col.header, width = col.width);
        }
        io.write_fmt(format_args!("{}\r\n", crate::ecma48::bold(&line)));
        
        // Underline
        let mut sep: String = String::new();
        for col in self.cols {
            for _ in 0..col.width {
                let _ = sep.push('-');
            }
            let _ = sep.push_str("  ");
        }
        io.write_fmt(format_args!("{}\r\n", crate::ecma48::dim(&sep)));
    }

    pub fn print_row<I, S>(&self, io: &dyn ShellIo, fields: I) 
    where 
        I: IntoIterator<Item = S>,
        S: core::fmt::Display,
    {
        let mut line: String = String::new();
        for (i, field) in fields.into_iter().enumerate() {
            if i >= self.cols.len() {
                break;
            }
            let width = self.cols[i].width;
            
            // Simple truncation/padding logic or use format!
            // Note: format! with dynamic width in no_std/alloc environment works usually.
            // We use a temporary string to measure/truncate if needed.
            
            let mut cell = String::new();
            let _ = write!(cell, "{}", field);
            
            if cell.len() > width {
                // Truncate
                let keep = width.saturating_sub(1);
                cell.truncate(keep);
                let _ = cell.push('…');
            }
            
            let _ = write!(line, "{:width$}  ", cell, width = width);
        }
        io.write_str(&line);
        io.write_str("\r\n");
    }
}
