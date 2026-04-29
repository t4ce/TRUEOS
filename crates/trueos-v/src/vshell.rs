extern crate alloc;

use alloc::{string::String, vec};
use core::fmt;

use crate::vcabi;

pub const RESET: &str = "\x1b[0m";
pub const CLEAR_LINE: &str = "\x1b[2K";
pub const CLEAR_TO_EOL: &str = "\x1b[K";
pub const BOLD: &str = "\x1b[1m";
pub const DIM: &str = "\x1b[2m";
pub const ITALIC: &str = "\x1b[3m";
pub const UNDERLINE: &str = "\x1b[4m";
pub const INVERT: &str = "\x1b[7m";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
}

pub mod color {
    use super::Rgb;

    pub const OK: Rgb = Rgb::new(60, 183, 161);
    pub const INFO: Rgb = Rgb::new(96, 165, 250);
    pub const WARN: Rgb = Rgb::new(245, 158, 11);
    pub const ERROR: Rgb = Rgb::new(248, 113, 113);
    pub const ACCENT: Rgb = Rgb::new(255, 55, 255);
    pub const MUTED: Rgb = Rgb::new(148, 163, 184);
    pub const WHITE: Rgb = Rgb::new(255, 255, 255);
}

#[derive(Clone, Copy)]
pub struct Style<'a> {
    text: &'a str,
    bold: bool,
    dim: bool,
    italic: bool,
    underline: bool,
    invert: bool,
    fg_rgb: Option<Rgb>,
    bg_rgb: Option<Rgb>,
    fg_ansi: Option<u8>,
    bg_ansi: Option<u8>,
}

impl fmt::Display for Style<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        #[inline]
        fn emit_code(f: &mut fmt::Formatter<'_>, first: &mut bool, code: &str) -> fmt::Result {
            if !*first {
                write!(f, ";")?;
            }
            *first = false;
            write!(f, "{}", code)
        }

        write!(f, "\x1b[")?;
        let mut first = true;

        if self.bold {
            emit_code(f, &mut first, "1")?;
        }
        if self.dim {
            emit_code(f, &mut first, "2")?;
        }
        if self.italic {
            emit_code(f, &mut first, "3")?;
        }
        if self.underline {
            emit_code(f, &mut first, "4")?;
        }
        if self.invert {
            emit_code(f, &mut first, "7")?;
        }
        if let Some(rgb) = self.fg_rgb {
            emit_code(f, &mut first, "38")?;
            emit_code(f, &mut first, "2")?;
            emit_code(f, &mut first, &alloc::format!("{}", rgb.r))?;
            emit_code(f, &mut first, &alloc::format!("{}", rgb.g))?;
            emit_code(f, &mut first, &alloc::format!("{}", rgb.b))?;
        } else if let Some(idx) = self.fg_ansi {
            emit_code(f, &mut first, "38")?;
            emit_code(f, &mut first, "5")?;
            emit_code(f, &mut first, &alloc::format!("{}", idx))?;
        }
        if let Some(rgb) = self.bg_rgb {
            emit_code(f, &mut first, "48")?;
            emit_code(f, &mut first, "2")?;
            emit_code(f, &mut first, &alloc::format!("{}", rgb.r))?;
            emit_code(f, &mut first, &alloc::format!("{}", rgb.g))?;
            emit_code(f, &mut first, &alloc::format!("{}", rgb.b))?;
        } else if let Some(idx) = self.bg_ansi {
            emit_code(f, &mut first, "48")?;
            emit_code(f, &mut first, "5")?;
            emit_code(f, &mut first, &alloc::format!("{}", idx))?;
        }
        if first {
            emit_code(f, &mut first, "0")?;
        }

        write!(f, "m{}{}", self.text, RESET)
    }
}

impl<'a> Style<'a> {
    pub fn bold(mut self) -> Self {
        self.bold = true;
        self
    }

    pub fn dim(mut self) -> Self {
        self.dim = true;
        self
    }

    pub fn italic(mut self) -> Self {
        self.italic = true;
        self
    }

    pub fn underline(mut self) -> Self {
        self.underline = true;
        self
    }

    pub fn invert(mut self) -> Self {
        self.invert = true;
        self
    }

    pub fn fg(mut self, rgb: Rgb) -> Self {
        self.fg_rgb = Some(rgb);
        self.fg_ansi = None;
        self
    }

    pub fn bg(mut self, rgb: Rgb) -> Self {
        self.bg_rgb = Some(rgb);
        self.bg_ansi = None;
        self
    }

    pub fn fg8(mut self, idx: u8) -> Self {
        self.fg_ansi = Some(idx);
        self.fg_rgb = None;
        self
    }

    pub fn bg8(mut self, idx: u8) -> Self {
        self.bg_ansi = Some(idx);
        self.bg_rgb = None;
        self
    }
}

pub fn style(text: &str) -> Style<'_> {
    Style {
        text,
        bold: false,
        dim: false,
        italic: false,
        underline: false,
        invert: false,
        fg_rgb: None,
        bg_rgb: None,
        fg_ansi: None,
        bg_ansi: None,
    }
}

#[inline]
pub fn uart1_shell_write(bytes: &[u8]) -> usize {
    if bytes.is_empty() {
        return 0;
    }
    unsafe { vcabi::trueos_cabi_uart1_shell_write(bytes.as_ptr(), bytes.len()) }
}

#[inline]
pub fn shell2_print_line(bytes: &[u8]) -> usize {
    if bytes.is_empty() {
        return 0;
    }
    unsafe { vcabi::trueos_cabi_shell2_print_line(bytes.as_ptr(), bytes.len()) }
}

#[inline]
pub fn line(text: &str) -> usize {
    shell2_print_line(text.as_bytes())
}

#[inline]
pub fn linef(args: fmt::Arguments<'_>) -> usize {
    let text = alloc::format!("{}", args);
    line(text.as_str())
}

#[inline]
pub fn styled_linef(args: fmt::Arguments<'_>, fg: Rgb, bold: bool) -> usize {
    let text = alloc::format!("{}", args);
    let styled = if bold {
        alloc::format!("{}", style(text.as_str()).fg(fg).bold())
    } else {
        alloc::format!("{}", style(text.as_str()).fg(fg))
    };
    line(styled.as_str())
}

#[inline]
pub fn ok(text: &str) -> usize {
    linef(format_args!("{}", style(text).fg(color::OK)))
}

#[inline]
pub fn info(text: &str) -> usize {
    linef(format_args!("{}", style(text).fg(color::INFO)))
}

#[inline]
pub fn warn(text: &str) -> usize {
    linef(format_args!("{}", style(text).fg(color::WARN).bold()))
}

#[inline]
pub fn error(text: &str) -> usize {
    linef(format_args!("{}", style(text).fg(color::ERROR).bold()))
}

#[inline]
pub fn shell1_submit_input(bytes: &[u8]) -> usize {
    if bytes.is_empty() {
        return 0;
    }
    unsafe { vcabi::trueos_cabi_shell1_submit_input(bytes.as_ptr(), bytes.len()) }
}

#[inline]
pub fn shell_command_registry_json() -> Option<String> {
    let len = unsafe { vcabi::trueos_cabi_shell_command_registry_json(core::ptr::null_mut(), 0) };
    if len <= 0 {
        return None;
    }

    let mut bytes = vec![0u8; len as usize];
    let got =
        unsafe { vcabi::trueos_cabi_shell_command_registry_json(bytes.as_mut_ptr(), bytes.len()) };
    if got <= 0 {
        return None;
    }
    bytes.truncate(got as usize);
    String::from_utf8(bytes).ok()
}

#[inline]
pub fn shell1_history_total_lines() -> usize {
    unsafe { vcabi::trueos_cabi_shell_history_lines_all() }
}

#[inline]
pub fn shell1_history_text_since(start_line: usize, max_lines: usize) -> Option<String> {
    let len = unsafe {
        vcabi::trueos_cabi_shell_history_lines(start_line, max_lines, core::ptr::null_mut(), 0)
    };
    if len <= 0 {
        return None;
    }

    let mut bytes = vec![0u8; len as usize];
    let got = unsafe {
        vcabi::trueos_cabi_shell_history_lines(
            start_line,
            max_lines,
            bytes.as_mut_ptr(),
            bytes.len(),
        )
    };
    if got <= 0 {
        return None;
    }
    bytes.truncate(got as usize);
    String::from_utf8(bytes).ok()
}
