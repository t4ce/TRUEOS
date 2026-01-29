use core::ffi::c_char;
use core::fmt::Write;

use alloc::vec::Vec;
use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use heapless::String;

use crate::disc::block;
use crate::shell::shellcube::{CubeState, WireShape, CUBE_COLS, CUBE_ROWS};

pub(crate) mod ecma48;

pub(crate) mod shellcube;
pub(crate) mod shellqjs;
pub(crate) mod txtedt;

pub(crate) mod cmd;

pub(crate) mod matrix;

mod crlf;

mod interface;
pub(crate) use interface::{ShellBackend, ShellIo};

pub(crate) mod backends;
pub(crate) use backends::{
    NetTcpShellBackend, Uart1Com1Backend, NET_TCP_SHELL_BACKEND,
    UART1_COM1_BACKEND
};

pub(crate) mod uart1_com1;

struct Utf8Decoder {
    buf: [u8; 4],
    len: usize,
    need: usize,
}

impl Utf8Decoder {
    const fn new() -> Self {
        Self {
            buf: [0u8; 4],
            len: 0,
            need: 0,
        }
    }

    fn clear(&mut self) {
        self.len = 0;
        self.need = 0;
    }

    fn push(&mut self, b: u8) -> Option<char> {
        if self.len == 0 {
            if b < 0x80 {
                return Some(b as char);
            }
            let need = match b {
                0xC2..=0xDF => 2,
                0xE0..=0xEF => 3,
                0xF0..=0xF4 => 4,
                _ => {
                    // Invalid leading byte.
                    return None;
                }
            };
            self.buf[0] = b;
            self.len = 1;
            self.need = need;
            return None;
        }

        // Continuation byte.
        if (b & 0xC0) != 0x80 {
            self.clear();
            return None;
        }

        self.buf[self.len] = b;
        self.len += 1;
        if self.len < self.need {
            return None;
        }

        let s = core::str::from_utf8(&self.buf[..self.need]).ok()?;
        let ch = s.chars().next()?;
        self.clear();
        Some(ch)
    }
}

const PROMPT_RGB: (u8, u8, u8) = (255, 55, 255);
const MATRIX_RUNNING_GLYPH: char = '⣿';
const DEFAULT_TERM_COLS: usize = 80;
const DEFAULT_TERM_ROWS: usize = 24;

#[inline]
fn write_prompt(io: &dyn ShellIo) {
    io.write_fmt(format_args!("{}", crate::ecma48::color("§ ", PROMPT_RGB)));
}

#[inline]
fn write_prompt_for_state(
    io: &dyn ShellIo,
    pending_action: Option<PendingAction>,
    install_wizard: Option<InstallWizardStage>,
) {
    // When we are actively asking the user for wizard/confirmation input,
    // avoid printing any prompt prefix so the user's typed input starts at
    // column 0 (less confusing in transcripts).
    if pending_action.is_some() || install_wizard.is_some() {
        return;
    }

    write_prompt(io);
}

#[inline]
fn write_right_aligned(io: &dyn ShellIo, row: usize, term_cols: usize, text: &str) {
    if term_cols == 0 || text.is_empty() {
        return;
    }
    let len = text.chars().count();
    let col = term_cols.saturating_sub(len).saturating_add(1);
    io.write_fmt(format_args!("{}", crate::ecma48::pos(row, col)));
    io.write_str(text);
}

#[inline]
fn write_banner(io: &dyn ShellIo, term_cols: usize) {
    // Row 1: Banner + matrix symbols on the right.
    io.write_fmt(format_args!("{}\n", crate::ecma48::bold("TRUE OS")));
    crate::matrix::refresh_matrix_symbols(io, term_cols);

    write_prompt(io);

    io.write_str(crate::ecma48::SAVE_CURSOR);

    // Banner command list: derive from the registry (single source of truth).
    // Keep it sorted for stable UI.
    let mut cmds: heapless::Vec<&'static str, 64> = heapless::Vec::new();
    crate::shell::cmd::list_command_names(&mut cmds);
    cmds.as_mut_slice().sort_unstable();

    for (idx, cmd) in cmds.iter().enumerate() {
        // Start at row 2 so row 1 stays reserved for banner + symbols.
        let row = idx + 2;
        write_right_aligned(io, row, term_cols, cmd);
    }
    io.write_str(crate::ecma48::RESTORE_CURSOR);
}

const GO_CHARS: [char; 9] = ['⣿', '⣾', '⣽', '⣻', '⢿', '⡿', '⣟', '⣯', '⣷'];

#[inline]
fn set_go_mode(io: &dyn ShellIo, go_mode: &mut bool, enable: bool) {
    let prev = *go_mode;
    if enable && !prev {
        io.write_str(crate::ecma48::HIDE_CURSOR);
    } else if !enable && prev {
        io.write_str(crate::ecma48::SHOW_CURSOR);
    }
    *go_mode = enable;
}

fn parse_disc_id_raw(s: &str) -> Option<u32> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let s = s.strip_prefix("disc").unwrap_or(s);
    s.parse::<u32>().ok()
}

fn print_install_disk_table(io: &dyn ShellIo) {
    io.write_str("install: disk detection stage\r\n");
    io.write_str("install: choose a disk id to continue (blank/q cancels)\r\n");
    io.write_str("\r\n");

    for h in crate::disc::block::device_handles().into_iter() {
        if h.parent().is_some() {
            continue;
        }
        let info = h.info();
        let (status, err) = crate::disc::detect::detect_physical_disk_detail(h);
        io.write_fmt(format_args!(
            "  id={} ({}) blocks={} bs={} writable={} label={:?} status={}{}\r\n",
            info.id.raw(),
            info.id,
            info.block_count,
            info.block_size,
            info.writable,
            info.label,
            status.short(),
            match (&status, err) {
                (crate::disc::detect::DiscStatus::Unknown, Some(e)) => {
                    // Keep it short; this is mainly for debugging why detection fails.
                    alloc::format!(" (err={:?})", e)
                }
                _ => alloc::string::String::new(),
            }
        ));
    }

    io.write_str("\r\n");
}

#[derive(Copy, Clone)]
enum PendingAction {
    Reset,
    S5,
    FormatConfirm { disc_id: u32 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InstallWizardStage {
    SelectDisk,
}

enum CommandAction {
    None,
    Pending(PendingAction),
    EnterCube,
    EnterIco,
    EnterTxtEdt { filename: String<48>, slot_id: u8 },
}



#[embassy_executor::task(pool_size = 3)]
pub async fn task(spawner: Spawner, io: &'static dyn ShellBackend) {
    io.init();

    // Ensure the registry is populated before the shell starts.
    self::cmd::init_builtin_shell_commands();

    let mut term_cols: usize = DEFAULT_TERM_COLS;
    let mut term_rows: usize = DEFAULT_TERM_ROWS;

    write_banner(io, term_cols);

    let mut line: String<128> = String::new();
    let mut utf8 = Utf8Decoder::new();
    let mut go_idx: usize = 0;
    let mut next_matrix_refresh: Instant = Instant::now() + EmbassyDuration::from_millis(250);
    let mut pending_action: Option<PendingAction> = None;
    let mut pending_deadline: Option<Instant> = None;
    let mut install_wizard: Option<InstallWizardStage> = None;
    let mut go_mode: bool = false;
    let mut cube_mode = true;
    let mut cube = CubeState::new();
    cube.set_shape(WireShape::Cube);
    cube.reset();
    enter_cube_mode(io, &mut term_cols, &mut term_rows);

    // Treat CRLF as a single Enter (common on serial/USB bridges).
    let mut saw_cr: bool = false;

    loop {
        if let Some(b) = io.read_byte() {
            if saw_cr && b == b'\n' {
                saw_cr = false;
                continue;
            }
            saw_cr = b == b'\r';
            if cube_mode {
                if b == b'\r' || b == b'\n' {
                    cube_mode = false;
                    set_go_mode(io, &mut go_mode, false);
                    io.write_str(crate::ecma48::CLEAR_SCREEN);
                    io.write_str(crate::ecma48::HOME);
                    write_banner(io, term_cols);
                }
                continue;
            }
            match b {
                b'\r' | b'\n' | b' ' if matches!(pending_action, Some(PendingAction::Reset | PendingAction::S5)) => {
                    utf8.clear();
                    // Other pending actions: Enter/Space cancels.
                    pending_action = None;
                    pending_deadline = None;
                    set_go_mode(io, &mut go_mode, false);
                    line.clear();
                    io.write_str("\r\n");
                    write_prompt_for_state(io, pending_action, install_wizard);
                    continue;
                }
                b'\r' | b'\n' => {
                    utf8.clear();

                    // Confirmation gate for destructive `format`.
                    if let Some(PendingAction::FormatConfirm { disc_id }) = pending_action {
                        let do_format = line.as_str().trim() == "FORMAT";
                        line.clear();
                        pending_action = None;
                        pending_deadline = None;
                        set_go_mode(io, &mut go_mode, false);

                        if do_format {
                            let target = crate::disc::block::device_handles()
                                .into_iter()
                                .find(|h| h.parent().is_none() && h.id().raw() == disc_id);
                            let Some(handle) = target else {
                                io.write_str("\r\nformat: no such disk\r\n");
                                write_prompt(io);
                                continue;
                            };

                            io.write_str("\r\nformat: writing TRUEOSFS...\r\n");
                            match crate::disc::trueosfs::format_blank(handle) {
                                Ok(()) => {
                                    let (status, err) = crate::disc::detect::detect_physical_disk_detail(handle);
                                    io.write_fmt(format_args!(
                                        "format: ok (status now: {}{})\r\n",
                                        status.short(),
                                        match (&status, err) {
                                            (crate::disc::detect::DiscStatus::Unknown, Some(e)) => alloc::format!("; err={:?}", e),
                                            _ => alloc::string::String::new(),
                                        }
                                    ));
                                }
                                Err(e) => {
                                    io.write_fmt(format_args!("format: failed ({:?})\r\n", e));
                                }
                            }
                        } else {
                            io.write_str("\r\nformat: cancelled\r\n");
                        }

                        write_prompt_for_state(io, pending_action, install_wizard);
                        continue;
                    }

                    // Interactive install wizard consumes whole-line input (including empty line).
                    if pending_action.is_none() {
                        if let Some(InstallWizardStage::SelectDisk) = install_wizard {
                            let mut s = line.as_str().trim();
                            // Accept inputs like `install 1` as well as just `1`.
                            if let Some(rest) = s.strip_prefix("install") {
                                s = rest.trim();
                            }
                            if s.is_empty() || s.eq_ignore_ascii_case("q") || s.eq_ignore_ascii_case("quit") {
                                line.clear();
                                install_wizard = None;
                                io.write_str("\r\ninstall: cancelled\r\n");
                                write_prompt_for_state(io, pending_action, install_wizard);
                                continue;
                            }

                            let raw_id = parse_disc_id_raw(s).unwrap_or(0);
                            line.clear();
                            if raw_id == 0 {
                                io.write_str("\r\ninstall: invalid id\r\n");
                                io.write_str("install: enter a disk id (e.g. 1 or disc001) or 'q'\r\n");
                                write_prompt_for_state(io, pending_action, install_wizard);
                                continue;
                            }

                            let target = crate::disc::block::device_handles()
                                .into_iter()
                                .find(|h| h.parent().is_none() && h.id().raw() == raw_id);
                            let Some(handle) = target else {
                                io.write_str("\r\ninstall: no such disk\r\n");
                                write_prompt_for_state(io, pending_action, install_wizard);
                                continue;
                            };

                            let status = crate::disc::detect::detect_physical_disk(handle);

                            let Some(kernel) = crate::limine::install_kernel_bytes() else {
                                io.write_str("\r\ninstall: kernel module missing\r\n");
                                io.write_str(
                                    "install: expected Limine module_string trueos.install.kernel\r\n",
                                );
                                install_wizard = None;
                                write_prompt_for_state(io, pending_action, install_wizard);
                                continue;
                            };

                            let Some(bootx64) = crate::limine::install_bootx64_bytes() else {
                                io.write_str("\r\ninstall: BOOTX64.EFI module missing\r\n");
                                io.write_str(
                                    "install: expected Limine module_string trueos.install.bootx64\r\n",
                                );
                                install_wizard = None;
                                write_prompt_for_state(io, pending_action, install_wizard);
                                continue;
                            };

                            io.write_fmt(format_args!(
                                "\r\ninstall: target status: {}\r\n",
                                status.short()
                            ));
                            match crate::matrix::alloc_slot(alloc::format!("install disc{:03}", raw_id).as_str()) {
                                Some(slot) => {
                                    let _ = spawner.spawn(crate::matrix::install_matrix_job(
                                        slot,
                                        handle,
                                        bootx64,
                                        kernel,
                                    ));
                                    io.write_fmt(format_args!(
                                        "install: started §{} (dump logs with § {})\r\n",
                                        slot + 1,
                                        slot + 1
                                    ));
                                    crate::matrix::refresh_matrix_symbols(io, term_cols);
                                }
                                None => {
                                    io.write_str("install: matrix full\r\n");
                                }
                            }

                            install_wizard = None;
                            write_prompt_for_state(io, pending_action, install_wizard);
                            continue;
                        }
                    }

                    if line.is_empty() && pending_action.is_none() && go_mode {
                        set_go_mode(io, &mut go_mode, false);
                        io.write_str("\r\n");
                        write_prompt_for_state(io, pending_action, install_wizard);
                        continue;
                    }
                    if line.is_empty() && pending_action.is_none() {
                        // Empty command: do nothing (no newline, no prompt).
                        continue;
                    }
                    if !line.is_empty() {
                        io.write_str("\r\n");
                        let action = handle_line(
                            &line,
                            &spawner,
                            io,
                            &mut term_cols,
                            &mut term_rows,
                            &mut go_mode,
                            &mut install_wizard,
                        );
                        line.clear();
                        match action {
                            CommandAction::Pending(action) => {
                                pending_action = Some(action);
                                pending_deadline = match action {
                                    PendingAction::Reset | PendingAction::S5 => {
                                        Some(Instant::now() + EmbassyDuration::from_secs(5))
                                    }
                                    PendingAction::FormatConfirm { .. } => None,
                                };
                                set_go_mode(
                                    io,
                                    &mut go_mode,
                                    matches!(action, PendingAction::Reset | PendingAction::S5),
                                );
                                write_prompt_for_state(io, pending_action, install_wizard);
                            }
                            CommandAction::EnterCube => {
                                cube_mode = true;
                                set_go_mode(io, &mut go_mode, false);
                                cube.set_shape(WireShape::Cube);
                                cube.reset();
                                enter_cube_mode(io, &mut term_cols, &mut term_rows);
                            }
                            CommandAction::EnterIco => {
                                cube_mode = true;
                                set_go_mode(io, &mut go_mode, false);
                                cube.set_shape(WireShape::Icosidodecahedron);
                                cube.reset();
                                enter_cube_mode(io, &mut term_cols, &mut term_rows);
                            }
                            CommandAction::EnterTxtEdt { filename, slot_id } => {
                                cube_mode = false;
                                set_go_mode(io, &mut go_mode, false);
                                let cols = term_cols;
                                let rows = term_rows;

                                // Edit the slot blob in-place (no auto-capture into a new slot).
                                let Some(buf) = crate::matrix::take_blob(slot_id) else {
                                    io.write_str("\r\ntxt: invalid slot\r\n");
                                    io.write_str(crate::ecma48::CLEAR_SCREEN);
                                    io.write_str(crate::ecma48::HOME);
                                    write_banner(io, term_cols);
                                    continue;
                                };

                                crate::matrix::set_state(slot_id, crate::matrix::SlotState::Running);
                                let out_buf = crate::shell::txtedt::run(io, cols, rows, filename.as_str(), buf).await;
                                let _ = crate::matrix::set_blob_owned_with_preview(slot_id, out_buf);
                                crate::matrix::set_state(slot_id, crate::matrix::SlotState::Done);
                                io.write_fmt(format_args!("\r\ntxt: updated §{}\r\n", slot_id + 1));
                                crate::matrix::refresh_matrix_symbols(io, term_cols);

                                io.write_str(crate::ecma48::CLEAR_SCREEN);
                                io.write_str(crate::ecma48::HOME);
                                write_banner(io, term_cols);
                            }
                            CommandAction::None => {
                                write_prompt_for_state(io, pending_action, install_wizard);
                            }
                        }
                    }
                }
                0x08 | 0x7F => {
                    utf8.clear();
                    if !line.is_empty() {
                        line.pop();
                        io.write_str("\x08 \x08");
                    }
                }
                0x03 => {
                    utf8.clear();
                    line.clear();
                    io.write_str("^C\r\n");
                    write_prompt(io);
                }
                _ => {
                    if b >= 0x20 {
                        if let Some(ch) = utf8.push(b) {
                            if line.push(ch).is_ok() {
                                io.write_char(ch);
                            }
                        }
                    }
                }
            }
        } else {
            if cube_mode {
                cube.draw_frame(io);
                Timer::after(EmbassyDuration::from_millis(333)).await;
                continue;
            }

            // Keep header symbols in sync with background job state transitions.
            if Instant::now() >= next_matrix_refresh {
                crate::matrix::refresh_matrix_symbols(io, term_cols);
                next_matrix_refresh = Instant::now() + EmbassyDuration::from_millis(250);
            }

            if let (Some(action), Some(deadline)) = (pending_action, pending_deadline) {
                if Instant::now() >= deadline {
                    set_go_mode(io, &mut go_mode, false);
                    pending_action = None;
                    pending_deadline = None;
                    match action {
                        PendingAction::Reset => {
                            if let Err(_err) = crate::efi::acpi::facp::reset_system() {
                                io.write_str("tlb miss warn\r\n");
                                write_prompt(io);
                            }
                        }
                        PendingAction::S5 => {
                            if crate::efi::acpi::facp::enter_s5(0, None).is_err() {
                                io.write_str("\r\ns5 failed\r\n");
                                write_prompt(io);
                            }
                        }
                        PendingAction::FormatConfirm { .. } => {}
                    }
                    continue;
                }
            }
            if go_mode {
                let ch = GO_CHARS[go_idx];
                go_idx = (go_idx + 1) % GO_CHARS.len();
                io.write_str("\r");
                write_prompt(io);
                io.write_char(ch);
                Timer::after(EmbassyDuration::from_millis(160)).await;
            } else {
                Timer::after(EmbassyDuration::from_millis(2)).await;
            }
        }
    }
}

fn handle_line(
    line: &str,
    spawner: &Spawner,
    io: &'static dyn ShellBackend,
    term_cols: &mut usize,
    term_rows: &mut usize,
    go_mode: &mut bool,
    install_wizard: &mut Option<InstallWizardStage>,
) -> CommandAction {
    let cmd = line.trim();
    if cmd.is_empty() {
        return CommandAction::None;
    }

    // New-style registered commands (typed args + validation + introspection).
    {
        let mut ctx = crate::shell::cmd::ShellCommandCtx {
            line: cmd,
            spawner,
            io,
            term_cols,
            term_rows,
            go_mode,
            install_wizard,
        };
        if let Some(action) = crate::shell::cmd::dispatch_line(&mut ctx) {
            return action;
        }
    }

    io.write_str("unknown: ");
    io.write_str(cmd);
    io.write_str("\r\n");
    CommandAction::None
}

fn unix_timestamp_to_ymdhms(ts: u64) -> (u32, u8, u8, u8, u8, u8) {
    const SECS_PER_MIN: u64 = 60;
    const SECS_PER_HOUR: u64 = 60 * SECS_PER_MIN;
    const SECS_PER_DAY: u64 = 24 * SECS_PER_HOUR;

    let mut days = ts / SECS_PER_DAY;
    let mut rem = ts % SECS_PER_DAY;

    let hour = (rem / SECS_PER_HOUR) as u8;
    rem %= SECS_PER_HOUR;
    let minute = (rem / SECS_PER_MIN) as u8;
    let second = (rem % SECS_PER_MIN) as u8;

    let mut year: u32 = 1970;
    loop {
        let days_in_year = if is_leap_year(year) { 366u64 } else { 365u64 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }

    let month_lengths = month_lengths(year);
    let mut month_idx = 0;
    while month_idx < month_lengths.len() {
        let len = month_lengths[month_idx] as u64;
        if days < len {
            let day = (days + 1) as u8;
            return (year, (month_idx + 1) as u8, day, hour, minute, second);
        }
        days -= len;
        month_idx += 1;
    }

    (year, 12, 31, hour, minute, second)
}

fn month_lengths(year: u32) -> [u8; 12] {
    if is_leap_year(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    }
}

fn is_leap_year(year: u32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

pub(crate) fn draw_corners(io: &dyn ShellIo, cols: usize, rows: usize) {
    if cols == 0 || rows == 0 {
        return;
    }
    io.write_str(crate::ecma48::SAVE_CURSOR);
    // top-right
    write_pos(io, 1, cols);
    io.write_byte(b'O');
    // bottom-left
    write_pos(io, rows, 1);
    io.write_byte(b'O');
    // bottom-right
    write_pos(io, rows, cols);
    io.write_byte(b'O');
    io.write_str(crate::ecma48::RESTORE_CURSOR);
}

#[inline]
fn write_pos(io: &dyn ShellIo, row: usize, col: usize) {
    io.write_fmt(format_args!("{}", crate::ecma48::pos(row, col)));
}

fn enter_cube_mode(io: &dyn ShellIo, term_cols: &mut usize, term_rows: &mut usize) {
    *term_cols = CUBE_COLS;
    *term_rows = CUBE_ROWS;
    draw_corners(io, CUBE_COLS, CUBE_ROWS);
    shellcube::enter_mode(io);
}
