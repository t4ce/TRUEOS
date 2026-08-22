extern crate alloc;

use alloc::string::String;
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
    let written = if bytes.is_empty() {
        0
    } else {
        unsafe { vcabi::trueos_cabi_shell_attached_write(bytes.as_ptr(), bytes.len()) }
    };
    let newline = b"\r\n";
    written.saturating_add(unsafe {
        vcabi::trueos_cabi_shell_attached_write(newline.as_ptr(), newline.len())
    })
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

#[derive(Debug, Eq, PartialEq)]
pub struct TerminalLease {
    epoch: u64,
}

#[derive(Debug, Eq, PartialEq)]
pub struct TerminalParkingTicket {
    ticket: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerminalSurfaceSnapshot {
    pub generation: u64,
    pub columns: u32,
    pub rows: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalLeaseError {
    Unsupported,
    NotActive,
    Stale,
    Detached,
    Busy,
    Transport(i32),
}

impl TerminalLeaseError {
    const fn from_rc(rc: i32) -> Self {
        match rc {
            -1 => Self::Unsupported,
            -2 => Self::NotActive,
            -3 => Self::Stale,
            -4 => Self::Detached,
            -5 => Self::Busy,
            other => Self::Transport(other),
        }
    }
}

impl fmt::Display for TerminalLeaseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => formatter.write_str("terminal handoff is unsupported"),
            Self::NotActive => formatter.write_str("terminal lease is not active"),
            Self::Stale => formatter.write_str("terminal lease epoch is stale"),
            Self::Detached => formatter.write_str("terminal console is detached"),
            Self::Busy => formatter.write_str("terminal ownership could not be transferred"),
            Self::Transport(code) => write!(formatter, "terminal lease transport error {code}"),
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum TerminalReentry {
    Pending,
    Ready(TerminalLease),
}

/// Atomically claim the launch-reserved terminal and return its first lease.
///
/// Until this call succeeds, Shell2 remains the visible/input owner and the
/// Blueprint may perform ordinary fallible initialization without blacking
/// out the launching terminal.
pub fn terminal_initial_lease() -> Result<TerminalLease, TerminalLeaseError> {
    let mut epoch = 0;
    let rc = unsafe { vcabi::trueos_cabi_blueprint_terminal_lease_current_v1(0, &mut epoch) };
    if rc == 0 && epoch != 0 {
        Ok(TerminalLease { epoch })
    } else {
        Err(TerminalLeaseError::from_rc(if rc == 0 { -6 } else { rc }))
    }
}

pub fn terminal_surface_snapshot() -> Result<TerminalSurfaceSnapshot, TerminalLeaseError> {
    let mut generation = 0;
    let mut columns = 0;
    let mut rows = 0;
    let rc = unsafe {
        vcabi::trueos_cabi_blueprint_terminal_surface_snapshot_v1(
            &mut generation,
            &mut columns,
            &mut rows,
        )
    };
    if rc == 0 && generation != 0 && columns != 0 && rows != 0 {
        Ok(TerminalSurfaceSnapshot {
            generation,
            columns,
            rows,
        })
    } else {
        Err(TerminalLeaseError::from_rc(if rc == 0 { -6 } else { rc }))
    }
}

impl TerminalLease {
    pub const fn epoch(&self) -> u64 {
        self.epoch
    }

    pub fn acknowledge_ready(&self) -> Result<(), TerminalLeaseError> {
        let mut observed = 0;
        let rc = unsafe {
            vcabi::trueos_cabi_blueprint_terminal_lease_current_v1(self.epoch, &mut observed)
        };
        if rc == 0 && observed == self.epoch {
            Ok(())
        } else if rc == 0 {
            Err(TerminalLeaseError::Stale)
        } else {
            Err(TerminalLeaseError::from_rc(rc))
        }
    }

    pub fn surface_snapshot(&self) -> Result<TerminalSurfaceSnapshot, TerminalLeaseError> {
        terminal_surface_snapshot()
    }

    pub fn release_to_shell(self) -> Result<TerminalParkingTicket, TerminalLeaseError> {
        let mut ticket = 0;
        let rc = unsafe {
            vcabi::trueos_cabi_blueprint_terminal_lease_release_v1(self.epoch, &mut ticket)
        };
        if rc == 0 && ticket != 0 {
            Ok(TerminalParkingTicket { ticket })
        } else {
            Err(TerminalLeaseError::from_rc(if rc == 0 { -6 } else { rc }))
        }
    }
}

impl TerminalParkingTicket {
    pub const fn epoch(&self) -> u64 {
        self.ticket
    }

    pub fn poll_reentry(&self) -> Result<TerminalReentry, TerminalLeaseError> {
        let mut epoch = 0;
        let rc = unsafe {
            vcabi::trueos_cabi_blueprint_terminal_lease_poll_reentry_v1(self.ticket, &mut epoch)
        };
        match rc {
            1 => Ok(TerminalReentry::Pending),
            0 if epoch != 0 => Ok(TerminalReentry::Ready(TerminalLease { epoch })),
            0 => Err(TerminalLeaseError::Transport(-6)),
            other => Err(TerminalLeaseError::from_rc(other)),
        }
    }

    /// Convenience wait that yields the guest VCPU between typed polls.
    /// Callers that must perform work while hidden should drive
    /// [`Self::poll_reentry`] from their own loop or executor instead.
    pub fn wait_for_reentry(self) -> Result<TerminalLease, TerminalLeaseError> {
        loop {
            match self.poll_reentry()? {
                TerminalReentry::Pending => {
                    crate::vsys::poll_once();
                    crate::vsys::sleep_ms(10);
                }
                TerminalReentry::Ready(lease) => return Ok(lease),
            }
        }
    }
}

#[inline]
pub fn leave_terminal_handoff() {
    if let Ok(lease) = terminal_initial_lease()
        && lease.release_to_shell().is_ok()
    {
        return;
    }
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
