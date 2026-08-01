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
pub fn progress_line(text: &str) -> usize {
    let mut frame = String::with_capacity(text.len().saturating_add(1));
    frame.push('\r');
    frame.push_str(text);
    attached_write(frame.as_bytes())
}

#[inline]
pub fn progress_linef(args: fmt::Arguments<'_>) -> usize {
    let text = alloc::format!("{}", args);
    progress_line(text.as_str())
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
pub fn write(bytes: &[u8]) -> usize {
    if bytes.is_empty() {
        return 0;
    }
    unsafe { vcabi::trueos_cabi_shell_attached_write(bytes.as_ptr(), bytes.len()) }
}

#[inline]
pub fn attached_write(bytes: &[u8]) -> usize {
    write(bytes)
}

#[inline]
pub fn shell2_raw_write(bytes: &[u8]) -> usize {
    if bytes.is_empty() {
        return 0;
    }
    unsafe { vcabi::trueos_cabi_shell2_raw_write(bytes.as_ptr(), bytes.len()) }
}

pub const SHELL2_FRONTEND_READ_DROPPED: u32 = 1 << 0;
pub const SHELL2_FRONTEND_DIRECT_HANDOFF: u32 = 1 << 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Shell2FrontendRead {
    pub len: usize,
    pub epoch: u64,
    pub epoch_changed: bool,
    pub flags: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Shell2FrontendError(pub i32);

pub struct Shell2Frontend {
    read_seq: u64,
    epoch: u64,
    attached: bool,
}

impl Shell2Frontend {
    pub fn attach(cols: u32, rows: u32) -> Result<Self, Shell2FrontendError> {
        let rc = unsafe { vcabi::trueos_cabi_shell2_frontend_attach_v1(cols, rows) };
        if rc != 0 {
            return Err(Shell2FrontendError(rc));
        }
        Ok(Self {
            read_seq: 0,
            epoch: 0,
            attached: true,
        })
    }

    pub fn read(&mut self, out: &mut [u8]) -> Result<Shell2FrontendRead, Shell2FrontendError> {
        let mut next_seq = self.read_seq;
        let mut epoch = self.epoch;
        let mut flags = 0u32;
        let rc = unsafe {
            vcabi::trueos_cabi_shell2_frontend_read_v1(
                self.read_seq,
                out.as_mut_ptr(),
                out.len(),
                &mut next_seq,
                &mut epoch,
                &mut flags,
            )
        };
        if rc < 0 {
            return Err(Shell2FrontendError(rc as i32));
        }
        let len = rc as usize;
        if len > out.len() {
            return Err(Shell2FrontendError(-3));
        }
        let epoch_changed = self.epoch != 0 && self.epoch != epoch;
        self.read_seq = next_seq;
        self.epoch = epoch;
        Ok(Shell2FrontendRead {
            len,
            epoch,
            epoch_changed,
            flags,
        })
    }

    /// Submit one atomic frontend input operation. Call once per typed glyph,
    /// or once for an entire coalesced paste burst.
    pub fn submit_input(&self, bytes: &[u8]) -> Result<usize, Shell2FrontendError> {
        if bytes.is_empty() {
            return Ok(0);
        }
        let rc = unsafe {
            vcabi::trueos_cabi_shell2_frontend_submit_input_v1(bytes.as_ptr(), bytes.len())
        };
        if rc < 0 {
            Err(Shell2FrontendError(rc as i32))
        } else {
            Ok(rc as usize)
        }
    }

    pub fn detach(mut self) -> Result<(), Shell2FrontendError> {
        let rc = unsafe { vcabi::trueos_cabi_shell2_frontend_detach_v1() };
        if rc != 0 {
            return Err(Shell2FrontendError(rc));
        }
        self.attached = false;
        Ok(())
    }
}

impl Drop for Shell2Frontend {
    fn drop(&mut self) {
        if self.attached {
            let _ = unsafe { vcabi::trueos_cabi_shell2_frontend_detach_v1() };
            self.attached = false;
        }
    }
}

const QJS_WORKBENCH_RESPONSE_CAP: usize = 160 * 1024 - 56;
const QJS_WORKBENCH_RESULT_HEADER_LEN: usize = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum QjsWorkbenchMode {
    Auto = 0,
    Script = 1,
    Module = 2,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QjsWorkbenchEval {
    pub ok: bool,
    pub mode: QjsWorkbenchMode,
    pub eval_count: u64,
    pub text: String,
}

pub fn qjs_workbench_eval(
    source: &str,
    mode: QjsWorkbenchMode,
) -> Result<QjsWorkbenchEval, String> {
    if source.is_empty() {
        return Err("source is empty".into());
    }
    let mut response = vec![0u8; QJS_WORKBENCH_RESPONSE_CAP];
    let len = unsafe {
        vcabi::trueos_cabi_qjs_workbench_eval_v1(
            source.as_ptr(),
            source.len(),
            mode as u32,
            response.as_mut_ptr(),
            response.len(),
        )
    };
    if len < QJS_WORKBENCH_RESULT_HEADER_LEN as isize {
        return Err("QuickJS workbench bridge failed".into());
    }
    let len = len as usize;
    let actual_mode = match response[1] {
        2 => QjsWorkbenchMode::Module,
        _ => QjsWorkbenchMode::Script,
    };
    let eval_count = u64::from_le_bytes(
        response[2..10]
            .try_into()
            .map_err(|_| String::from("invalid QuickJS result header"))?,
    );
    let text =
        String::from_utf8_lossy(&response[QJS_WORKBENCH_RESULT_HEADER_LEN..len]).into_owned();
    match response[0] {
        0 | 1 => Ok(QjsWorkbenchEval {
            ok: response[0] == 0,
            mode: actual_mode,
            eval_count,
            text,
        }),
        _ => Err(text),
    }
}

pub fn qjs_workbench_poll() -> Result<String, String> {
    let mut response = vec![0u8; QJS_WORKBENCH_RESPONSE_CAP];
    let len =
        unsafe { vcabi::trueos_cabi_qjs_workbench_poll_v1(response.as_mut_ptr(), response.len()) };
    if len < 0 {
        return Err("QuickJS output poll failed".into());
    }
    Ok(String::from_utf8_lossy(&response[..len as usize]).into_owned())
}

pub fn qjs_workbench_close() {
    let _ = unsafe { vcabi::trueos_cabi_qjs_workbench_close_v1() };
}

#[inline]
pub fn leave_terminal_handoff() {
    let _ = unsafe { vcabi::trueos_cabi_blueprint_return_to_cli() };
}

#[inline]
pub fn attached_read_byte() -> Option<u8> {
    let value = unsafe { vcabi::trueos_cabi_shell_attached_read_byte() };
    if (0..=255).contains(&value) {
        Some(value as u8)
    } else {
        None
    }
}

const TERMINAL_REENTRY_BYTE: u8 = 0x1f;

pub fn wait_for_terminal_reentry() {
    loop {
        if attached_read_byte() == Some(TERMINAL_REENTRY_BYTE) {
            return;
        }
        crate::vsys::poll_once();
        crate::vsys::sleep_ms(5);
    }
}

#[inline]
pub fn read(buf: &mut [u8]) -> usize {
    let mut read = 0;
    for slot in buf {
        let Some(byte) = attached_read_byte() else {
            break;
        };
        *slot = byte;
        read += 1;
    }
    read
}

pub fn read_blocking(buf: &mut [u8]) -> usize {
    if buf.is_empty() {
        return 0;
    }

    loop {
        let read = read(buf);
        if read != 0 {
            return read;
        }
        crate::vsys::poll_once();
        crate::vsys::sleep_ms(100);
    }
}

#[inline]
pub fn attached_read_available(buf: &mut [u8]) -> usize {
    read(buf)
}

#[inline]
pub fn attached_retarget_slot(slot: &str) -> bool {
    if slot.is_empty() {
        return false;
    }
    unsafe { vcabi::trueos_cabi_shell_attached_retarget_slot(slot.as_ptr(), slot.len()) == 0 }
}

pub const KONSOLE_FRAME_TERMINAL_HANDOFF: u32 = 1 << 31;

#[inline]
pub fn konsole_begin_frame(cols: u32, rows: u32, reserved_top_rows: u32) -> i32 {
    unsafe { vcabi::trueos_cabi_konsole_begin_frame(cols, rows, reserved_top_rows) }
}

#[inline]
pub fn konsole_write_row(row: u32, col: u32, bytes: &[u8]) -> i32 {
    let ptr = if bytes.is_empty() {
        core::ptr::null()
    } else {
        bytes.as_ptr()
    };
    unsafe { vcabi::trueos_cabi_konsole_write_row(row, col, ptr, bytes.len()) }
}

#[inline]
pub fn konsole_set_cursor(row: u32, col: u32, visible: bool) -> i32 {
    unsafe { vcabi::trueos_cabi_konsole_set_cursor(row, col, u32::from(visible)) }
}

#[inline]
pub fn konsole_end_frame() -> i32 {
    unsafe { vcabi::trueos_cabi_konsole_end_frame() }
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
