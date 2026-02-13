use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;
use core::cell::RefCell;
use crate::shell::ShellIo;

pub struct TableColumn {
    pub header: &'static str,
    pub width: usize,
}

pub struct Table<'a, 'io> {
    cols: &'a [TableColumn],
    io: RefCell<Option<&'io dyn ShellIo>>,
    lines: RefCell<Vec<String>>,
}

impl<'a, 'io> Table<'a, 'io> {
    pub fn new(cols: &'a [TableColumn]) -> Self {
        Self { 
            cols,
            io: RefCell::new(None),
            lines: RefCell::new(Vec::new()),
        }
    }

    fn capture_io(&self, io: &'io dyn ShellIo) {
        let mut slot = self.io.borrow_mut();
        if slot.is_none() {
            *slot = Some(io);
        }
    }

    pub fn print_header(&self, io: &'io dyn ShellIo) {
        self.capture_io(io);
        let mut line: String = String::new();
        
        // Header row
        for col in self.cols {
            let _ = write!(line, "{:width$}  ", col.header, width = col.width);
        }
        
        let header_str = alloc::format!("{}\r\n", crate::ecma48::bold(&line));
        self.lines.borrow_mut().push(header_str);
        
        // Underline
        let mut sep: String = String::new();
        for col in self.cols {
            for _ in 0..col.width {
                let _ = sep.push('-');
            }
            let _ = sep.push_str("  ");
        }
        let sep_str = alloc::format!("{}\r\n", crate::ecma48::dim(&sep));
        self.lines.borrow_mut().push(sep_str);
    }

    pub fn print_row<I, S>(&self, io: &'io dyn ShellIo, fields: I) 
    where 
        I: IntoIterator<Item = S>,
        S: core::fmt::Display,
    {
        self.capture_io(io);
        let mut line: String = String::new();
        for (i, field) in fields.into_iter().enumerate() {
            if i >= self.cols.len() {
                break;
            }
            let width = self.cols[i].width;
            
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
        
        // self.lines.borrow_mut().push(line + "\r\n");
        let mut slot = self.lines.borrow_mut();
        slot.push(line);
        if let Some(last) = slot.last_mut() {
            let _ = last.push_str("\r\n");
        }
    }
}

impl<'a, 'io> Drop for Table<'a, 'io> {
    fn drop(&mut self) {
        let slot = self.io.borrow();
        if let Some(io) = *slot {
             // We print lines in REVERSE order because the shell pushes lines to the TOP.
             // Standard logical order: Header, Sep, Row0, Row1...
             // Stacked output: 
             //   Row 1 (last written) -> Top
             //   Row 0
             //   Sep
             //   Header (first written) -> Bottom
             //
             // Thus we iterate lines.rev()
             
             for line in self.lines.borrow().iter().rev() {
                 io.write_str(line);
             }
        }
    }
}

