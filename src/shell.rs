use core::fmt::Write;
use core::ffi::c_char;
use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use heapless::String;

use crate::shellcube::{CubeState, WireShape, CUBE_COLS, CUBE_ROWS};
use crate::ecma48;
use crate::disc::block;

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

const SHELL_COMMANDS: [&str; 19] = [
    "echo",
    "qjs",
    "qjsm",
    "files",
    "§",
    "s5",
    "reset",
    "install",
    "set",
    "go",
    "mandel",
    "time",
    "up",
    "idle",
    "pstate",
    "cube",
    "ico",
    "txt",
    "insane",
];

pub(crate) trait ShellIo {
    fn write_str(&self, s: &str);
    fn write_fmt(&self, args: core::fmt::Arguments<'_>);
    fn write_char(&self, ch: char);
    fn write_byte(&self, b: u8);
}

pub(crate) trait ShellBackend: ShellIo {
    fn init(&self) {}
    fn read_byte(&self) -> Option<u8>;
}

pub(crate) struct Uart1Com1Backend;

pub(crate) static UART1_COM1_BACKEND: Uart1Com1Backend = Uart1Com1Backend;

impl ShellIo for Uart1Com1Backend {
    #[inline]
    fn write_str(&self, s: &str) {
        uart1_com1::write_str(s);
    }

    #[inline]
    fn write_fmt(&self, args: core::fmt::Arguments<'_>) {
        uart1_com1::write_fmt(args);
    }

    #[inline]
    fn write_char(&self, ch: char) {
        uart1_com1::write_char(ch);
    }

    #[inline]
    fn write_byte(&self, b: u8) {
        uart1_com1::write_byte(b);
    }
}

impl ShellBackend for Uart1Com1Backend {
    #[inline]
    fn init(&self) {
        uart1_com1::init();
    }

    #[inline]
    fn read_byte(&self) -> Option<u8> {
        uart1_com1::read_byte()
    }
}

pub(crate) struct UsbCdcShellBackend;

pub(crate) static USB_CDC_SHELL_BACKEND: UsbCdcShellBackend = UsbCdcShellBackend;

pub(crate) struct NetTcpShellBackend;

pub(crate) static NET_TCP_SHELL_BACKEND: NetTcpShellBackend = NetTcpShellBackend;

impl ShellIo for UsbCdcShellBackend {
    #[inline]
    fn write_str(&self, s: &str) {
        let _ = crate::usb::cdc_shell::write(s.as_bytes());
    }

    #[inline]
    fn write_fmt(&self, args: core::fmt::Arguments<'_>) {
        use core::fmt::Write;
        struct Writer;
        impl Write for Writer {
            fn write_str(&mut self, s: &str) -> core::fmt::Result {
                let _ = crate::usb::cdc_shell::write(s.as_bytes());
                Ok(())
            }
        }
        let _ = Writer.write_fmt(args);
    }

    #[inline]
    fn write_char(&self, ch: char) {
        let mut buf = [0u8; 4];
        let s = ch.encode_utf8(&mut buf);
        let _ = crate::usb::cdc_shell::write(s.as_bytes());
    }

    #[inline]
    fn write_byte(&self, b: u8) {
        let _ = crate::usb::cdc_shell::write(&[b]);
    }
}

impl ShellBackend for UsbCdcShellBackend {
    #[inline]
    fn read_byte(&self) -> Option<u8> {
        crate::usb::cdc_shell::read_byte()
    }
}

impl ShellIo for NetTcpShellBackend {
    #[inline]
    fn write_str(&self, s: &str) {
        crate::net::adapter::net_shell_write_bytes(s.as_bytes());
    }

    #[inline]
    fn write_fmt(&self, args: core::fmt::Arguments<'_>) {
        struct Writer;
        impl Write for Writer {
            fn write_str(&mut self, s: &str) -> core::fmt::Result {
                crate::net::adapter::net_shell_write_bytes(s.as_bytes());
                Ok(())
            }
        }
        let _ = Writer.write_fmt(args);
    }

    #[inline]
    fn write_char(&self, ch: char) {
        let mut buf = [0u8; 4];
        let s = ch.encode_utf8(&mut buf);
        crate::net::adapter::net_shell_write_bytes(s.as_bytes());
    }

    #[inline]
    fn write_byte(&self, b: u8) {
        crate::net::adapter::net_shell_write_byte(b);
    }
}

impl ShellBackend for NetTcpShellBackend {
    #[inline]
    fn read_byte(&self) -> Option<u8> {
        crate::net::adapter::net_shell_read_byte()
    }
}

#[inline]
fn write_prompt(io: &dyn ShellIo) {
    io.write_fmt(format_args!("{}", crate::ecma48::color("§ ", PROMPT_RGB)));
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
fn refresh_matrix_symbols(io: &dyn ShellIo, term_cols: usize) {
    io.write_str(crate::ecma48::SAVE_CURSOR);
    let mut symbols: heapless::Vec<(u8, crate::matrix::SlotState), { crate::matrix::MAX_SLOTS }> =
        heapless::Vec::new();
    crate::matrix::collect_symbols(&mut symbols);

    let mut visible_len: usize = 0;
    for (i, (id, _state)) in symbols.iter().enumerate() {
        if i != 0 {
            visible_len += 1;
        }
        match _state {
            crate::matrix::SlotState::Running => {
                // "§⣿"
                visible_len += 2;
            }
            _ => {
                // "§<id>"
                visible_len += 1; // '§'
                let mut n = *id as usize;
                let mut digits = 1;
                while n >= 10 {
                    digits += 1;
                    n /= 10;
                }
                visible_len += digits;
            }
        }
    }

    // Clear the right-side symbol area first so shrinking/empty updates don't
    // leave stale characters behind.
    if term_cols != 0 {
        let clear_width: usize = 64;
        let mut start_col = term_cols.saturating_sub(clear_width).saturating_add(1);
        // Keep the left banner ("TRUE OS") intact.
        start_col = start_col.max(9);
        if start_col <= term_cols {
            io.write_fmt(format_args!("{}", crate::ecma48::pos(1, start_col)));
            let to_clear = term_cols - start_col + 1;
            for _ in 0..to_clear {
                io.write_byte(b' ');
            }
        }
    }

    if term_cols != 0 && visible_len != 0 {
        let mut start_col = term_cols.saturating_sub(visible_len).saturating_add(1);
        start_col = start_col.max(9);
        if start_col <= term_cols {
            io.write_fmt(format_args!("{}", crate::ecma48::pos(1, start_col)));
            for (i, (id, state)) in symbols.iter().enumerate() {
                if i != 0 {
                    io.write_byte(b' ');
                }
                match *state {
                    crate::matrix::SlotState::Running => {
                        let mut s: String<4> = String::new();
                        let _ = s.push('§');
                        let _ = s.push(MATRIX_RUNNING_GLYPH);
                        io.write_str(s.as_str());
                    }
                    _ => {
                        let mut s: String<8> = String::new();
                        let _ = write!(s, "§{}", id);
                        if *state == crate::matrix::SlotState::Done {
                            io.write_fmt(format_args!(
                                "{}",
                                crate::ecma48::color(s.as_str(), PROMPT_RGB)
                            ));
                        } else {
                            io.write_str(s.as_str());
                        }
                    }
                }
            }
        }
    }
    io.write_str(crate::ecma48::RESTORE_CURSOR);
}

#[inline]
fn write_banner(io: &dyn ShellIo, term_cols: usize) {
    // Row 1: Banner + matrix symbols on the right.
    io.write_fmt(format_args!("{}\n", crate::ecma48::bold("TRUE OS")));
    refresh_matrix_symbols(io, term_cols);

    write_prompt(io);

    io.write_str(crate::ecma48::SAVE_CURSOR);
    for (idx, cmd) in SHELL_COMMANDS.iter().enumerate() {
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
        io.write_str(ecma48::HIDE_CURSOR);
    } else if !enable && prev {
        io.write_str(ecma48::SHOW_CURSOR);
    }
    *go_mode = enable;
}

#[derive(Copy, Clone)]
enum PendingAction {
    Reset,
    S5,
    Install {
        raw_id: u32,
        migrate: bool,
    },
}

enum CommandAction {
    None,
    Pending(PendingAction),
    EnterCube,
    EnterIco,
    EnterTxtEdt { filename: String<48> },
}

#[embassy_executor::task]
pub async fn task(spawner: Spawner, io: &'static dyn ShellBackend) {
    io.init();

    let mut term_cols: usize = DEFAULT_TERM_COLS;
    let mut term_rows: usize = DEFAULT_TERM_ROWS;

    write_banner(io, term_cols);

    let mut line: String<128> = String::new();
    let mut utf8 = Utf8Decoder::new();
    let mut go_idx: usize = 0;
    let mut next_matrix_refresh: Instant = Instant::now() + EmbassyDuration::from_millis(250);
    let mut pending_action: Option<PendingAction> = None;
    let mut pending_deadline: Option<Instant> = None;
    let mut go_mode: bool = false;
    let mut cube_mode = true;
    let mut cube = CubeState::new();
    cube.set_shape(WireShape::Cube);
    cube.reset();
    enter_cube_mode(io, &mut term_cols, &mut term_rows);

    loop {
        if let Some(b) = io.read_byte() {
            if cube_mode {
                if b == b'\r' || b == b'\n' {
                    cube_mode = false;
                    set_go_mode(io, &mut go_mode, false);
                    io.write_str(ecma48::CLEAR_SCREEN);
                    io.write_str(ecma48::HOME);
                    write_banner(io, term_cols);
                }
                continue;
            }
            match b {
                b'\r' | b'\n' | b' ' if pending_action.is_some() => {
                    utf8.clear();
                    // Pending install confirmation: Enter = proceed, Space = abort.
                    if let Some(PendingAction::Install { raw_id, migrate }) = pending_action {
                        match b {
                            b'\r' | b'\n' => {
                                pending_action = None;
                                pending_deadline = None;
                                set_go_mode(io, &mut go_mode, false);
                                line.clear();
                                io.write_str("\r\n");
                                crate::install::run_install(io, raw_id, migrate);
                                write_prompt(io);
                            }
                            b' ' => {
                                pending_action = None;
                                pending_deadline = None;
                                set_go_mode(io, &mut go_mode, false);
                                line.clear();
                                io.write_str("\r\ninstall: aborted\r\n");
                                write_prompt(io);
                            }
                            _ => {}
                        }
                        continue;
                    }

                    // Other pending actions: Enter/Space cancels.
                    pending_action = None;
                    pending_deadline = None;
                    set_go_mode(io, &mut go_mode, false);
                    line.clear();
                    io.write_str("\r\n");
                    write_prompt(io);
                    continue;
                }
                b'\r' | b'\n' => {
                    utf8.clear();
                    if line.is_empty() && pending_action.is_none() && go_mode {
                        set_go_mode(io, &mut go_mode, false);
                        io.write_str("\r\n");
                        write_prompt(io);
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
                        );
                        line.clear();
                        write_prompt(io);
                        match action {
                            CommandAction::Pending(action) => {
                                pending_action = Some(action);
                                pending_deadline = match action {
                                    PendingAction::Reset | PendingAction::S5 => {
                                        Some(Instant::now() + EmbassyDuration::from_secs(5))
                                    }
                                    PendingAction::Install { .. } => None,
                                };
                                set_go_mode(
                                    io,
                                    &mut go_mode,
                                    matches!(action, PendingAction::Reset | PendingAction::S5),
                                );
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
                            CommandAction::EnterTxtEdt { filename } => {
                                cube_mode = false;
                                set_go_mode(io, &mut go_mode, false);
                                let cols = term_cols;
                                let rows = term_rows;
                                crate::txtedt::run(io, cols, rows, filename.as_str()).await;
                                io.write_str(ecma48::CLEAR_SCREEN);
                                io.write_str(ecma48::HOME);
                                write_banner(io, term_cols);
                            }
                            CommandAction::None => {}
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
                cube.draw_frame();
                Timer::after(EmbassyDuration::from_millis(333)).await;
                continue;
            }

            // Keep header symbols in sync with background job state transitions.
            if Instant::now() >= next_matrix_refresh {
                refresh_matrix_symbols(io, term_cols);
                next_matrix_refresh = Instant::now() + EmbassyDuration::from_millis(250);
            }

            if let (Some(action), Some(deadline)) = (pending_action, pending_deadline) {
                if Instant::now() >= deadline {
                    set_go_mode(io, &mut go_mode, false);
                    pending_action = None;
                    pending_deadline = None;
                    match action {
                        PendingAction::Reset => {
                            if let Err(err) = crate::acpi::facp::reset_system() {
                                io.write_str("tlb miss warn\r\n");
                                write_prompt(io);
                            }
                        }
                        PendingAction::S5 => {
                            if crate::acpi::facp::enter_s5(0, None).is_err() {
                                io.write_str("\r\ns5 failed\r\n");
                                write_prompt(io);
                            }
                        }
                        PendingAction::Install { .. } => {}
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

#[embassy_executor::task]
async fn files_matrix_job(slot_id: u8, start_seq: u32) {
    crate::matrix::push_line(slot_id, "queued scan request");
    crate::disc::files::request_files_scan();
    crate::matrix::push_line(slot_id, "waiting for scan...");

    let deadline = Instant::now() + EmbassyDuration::from_secs(10);
    loop {
        let now_seq = crate::disc::files::file_tree_seq();
        if now_seq != start_seq {
            let nodes = crate::disc::files::file_tree_len();
            crate::matrix::push_line(slot_id, "scan complete");
            let mut line: String<96> = String::new();
            let _ = write!(line, "seq={} nodes={}", now_seq, nodes);
            crate::matrix::push_line(slot_id, line.as_str());
            crate::matrix::set_state(slot_id, crate::matrix::SlotState::Done);
            break;
        }
        if Instant::now() >= deadline {
            crate::matrix::push_line(slot_id, "timeout waiting for scan");
            crate::matrix::set_state(slot_id, crate::matrix::SlotState::Failed);
            break;
        }
        Timer::after(EmbassyDuration::from_millis(100)).await;
    }
}

fn handle_line(
    line: &str,
    spawner: &Spawner,
    io: &'static dyn ShellBackend,
    term_cols: &mut usize,
    term_rows: &mut usize,
    go_mode: &mut bool,
) -> CommandAction {
    let cmd = line.trim();
    if cmd.is_empty() {
        return CommandAction::None;
    }

    // Shorthand: "§1" behaves like "§ 1" (dump + clear).
    if let Some(n) = cmd.strip_prefix('§') {
        if !n.is_empty() {
            if let Ok(id) = n.parse::<u8>() {
                if id == 0 {
                    io.write_str("§: ids are 1..\r\n");
                    return CommandAction::None;
                }
                let mut buf: String<1024> = String::new();
                if crate::matrix::dump_slot(&mut buf, id - 1) {
                    io.write_str(buf.as_str());
                    let _ = crate::matrix::free_slot(id - 1);
                    refresh_matrix_symbols(io, *term_cols);
                } else {
                    io.write_str("§: not found\r\n");
                }
                return CommandAction::None;
            }
        }
    }

    if let Some((verb, rest)) = cmd.split_once(' ') {
        if verb.eq_ignore_ascii_case("install") {
            if let Some(p) = crate::install::handle_install_command(io, rest) {
                return CommandAction::Pending(PendingAction::Install {
                    raw_id: p.raw_id,
                    migrate: p.migrate,
                });
            }
            return CommandAction::None;
        }
        if verb.eq_ignore_ascii_case("files") {
            let _ = rest;
            let seq = crate::disc::files::file_tree_seq();
            let nodes = crate::disc::files::file_tree_len();
            io.write_fmt(format_args!(
                "files: cache seq={} nodes={}\r\n",
                seq, nodes
            ));
            match crate::matrix::alloc_slot("files scan") {
                Some(slot) => {
                    crate::matrix::push_line(slot, "files: job started");
                    let mut line: String<64> = String::new();
                    let _ = write!(line, "start seq={}", seq);
                    crate::matrix::push_line(slot, line.as_str());
                    let _ = spawner.spawn(files_matrix_job(slot, seq));
                    io.write_fmt(format_args!("files: started §{}\r\n", slot + 1));
                    refresh_matrix_symbols(io, *term_cols);
                }
                None => {
                    io.write_str("files: matrix full\r\n");
                }
            }
            return CommandAction::None;
        }
        if verb == "§" {
            let mut arg = rest.trim();
            if arg.is_empty() {
                let mut buf: String<512> = String::new();
                crate::matrix::list_slots(&mut buf);
                io.write_str(buf.as_str());
                return CommandAction::None;
            }

            // Accept "§ §1" as well as "§ 1".
            if let Some(n) = arg.strip_prefix('§') {
                arg = n.trim();
            }

            if let Ok(id) = arg.parse::<u8>() {
                if id == 0 {
                    io.write_str("§: ids are 1..\r\n");
                    return CommandAction::None;
                }
                let mut buf: String<1024> = String::new();
                if crate::matrix::dump_slot(&mut buf, id - 1) {
                    io.write_str(buf.as_str());
                    let _ = crate::matrix::free_slot(id - 1);
                    refresh_matrix_symbols(io, *term_cols);
                } else {
                    io.write_str("§: not found\r\n");
                }
                return CommandAction::None;
            }

            io.write_str("§: usage § | § <id>\r\n");
            return CommandAction::None;
        }
        if verb.eq_ignore_ascii_case("qjs") {
            let src = rest.trim();
            if src.is_empty() {
                io.write_str("qjs: usage qjs <javascript>\r\n");
            } else {
                if let Some(path) = src.strip_prefix('@') {
                    let path = path.trim();
                    match crate::disc::files::Fs::read_file(path) {
                        Ok(bytes) => {
                            let flags = if path.ends_with(".mjs")
                                || crate::shellqjs::looks_like_module_bytes(&bytes)
                            {
                                trueos_qjs::JS_EVAL_TYPE_MODULE
                            } else {
                                trueos_qjs::JS_EVAL_TYPE_GLOBAL
                            };
                            let filename = if flags == trueos_qjs::JS_EVAL_TYPE_MODULE {
                                b"<shell-module-file>\0".as_ptr() as *const c_char
                            } else {
                                b"<shell-file>\0".as_ptr() as *const c_char
                            };
                            crate::shellqjs::eval_bytes(io, filename, &bytes, flags);
                        }
                        Err(e) => io.write_fmt(format_args!("qjs: read_file failed ({:?})\r\n", e)),
                    }
                } else {
                    crate::shellqjs::eval(io, src);
                }
            }
            return CommandAction::None;
        }
        if verb.eq_ignore_ascii_case("qjsm") {
            let src = rest.trim();
            if src.is_empty() {
                io.write_str("qjsm: usage qjsm <module-source>\r\n");
                io.write_str("qjsm: example qjsm import { make } from 'complex'; make(1,2)\r\n");
            } else {
                if let Some(path) = src.strip_prefix('@') {
                    let path = path.trim();
                    match crate::disc::files::Fs::read_file(path) {
                        Ok(bytes) => {
                            let filename = b"<shell-module-file>\0".as_ptr() as *const c_char;
                            crate::shellqjs::eval_bytes(io, filename, &bytes, trueos_qjs::JS_EVAL_TYPE_MODULE);
                        }
                        Err(e) => io.write_fmt(format_args!("qjsm: read_file failed ({:?})\r\n", e)),
                    }
                } else {
                    crate::shellqjs::eval_module(io, src);
                }
            }
            return CommandAction::None;
        }

        if verb.eq_ignore_ascii_case("txt") || verb.eq_ignore_ascii_case("txtedt") {
            let mut filename: String<48> = String::new();
            let name = rest.trim();
            let name = if name.is_empty() { "untitled.txt" } else { name };
            for ch in name.chars() {
                if filename.push(ch).is_err() {
                    break;
                }
            }
            return CommandAction::EnterTxtEdt { filename };
        }
    } else if cmd.eq_ignore_ascii_case("install") {
        if let Some(p) = crate::install::handle_install_command(io, "") {
            return CommandAction::Pending(PendingAction::Install {
                raw_id: p.raw_id,
                migrate: p.migrate,
            });
        }
        return CommandAction::None;
    } else if cmd.eq_ignore_ascii_case("files") {
        let seq = crate::disc::files::file_tree_seq();
        let nodes = crate::disc::files::file_tree_len();
        io.write_fmt(format_args!(
            "files: cache seq={} nodes={}\r\n",
            seq, nodes
        ));
        match crate::matrix::alloc_slot("files scan") {
            Some(slot) => {
                crate::matrix::push_line(slot, "files: job started");
                let mut line: String<64> = String::new();
                let _ = write!(line, "start seq={}", seq);
                crate::matrix::push_line(slot, line.as_str());
                let _ = spawner.spawn(files_matrix_job(slot, seq));
                io.write_fmt(format_args!("files: started §{}\r\n", slot + 1));
            }
            None => {
                io.write_str("files: matrix full\r\n");
            }
        }
        refresh_matrix_symbols(io, *term_cols);
        return CommandAction::None;
    } else if cmd.eq_ignore_ascii_case("qjs") {
        io.write_str("qjs: usage qjs <javascript>\r\n");
        io.write_str("qjs: example qjs print(1+2)\r\n");
        return CommandAction::None;
    } else if cmd == "§" {
        let mut buf: String<512> = String::new();
        crate::matrix::list_slots(&mut buf);
        io.write_str(buf.as_str());
        return CommandAction::None;
    } else if cmd.eq_ignore_ascii_case("qjsm") {
        io.write_str("qjsm: usage qjsm <module-source>\r\n");
        io.write_str("qjsm: example qjsm import { make } from 'complex'; make(1,2)\r\n");
        return CommandAction::None;
    }

    if let Some((cols, rows)) = parse_set_dims(cmd) {
        *term_cols = cols;
        *term_rows = rows;
        io.write_str("term set: ");
        write_usize(io, cols);
        io.write_str("x");
        write_usize(io, rows);
        io.write_str("\r\n");
        draw_corners(io, cols, rows);
        return CommandAction::None;
    }

    if cmd.eq_ignore_ascii_case("reset") {
        return CommandAction::Pending(PendingAction::Reset);
    }

    if cmd.eq_ignore_ascii_case("s5") {
        return CommandAction::Pending(PendingAction::S5);
    }

    if cmd.eq_ignore_ascii_case("go") {
        set_go_mode(io, go_mode, true);
        return CommandAction::None;
    }

    if cmd.eq_ignore_ascii_case("mandel") {
        crate::draw_mandelbrot();
        io.write_str("mandel ok\r\n");
        return CommandAction::None;
    }

    if cmd.eq_ignore_ascii_case("time") {
        let Some(boot_ts) = crate::limine::boot_timestamp_secs() else {
            io.write_str("time: boot timestamp unavailable\r\n");
            return CommandAction::None;
        };
        let now_ticks = embassy_time_driver::now();
        let elapsed_secs = now_ticks / (embassy_time_driver::TICK_HZ as u64);
        let ts = boot_ts.saturating_add(elapsed_secs);
        let (year, month, day, hour, minute, second) = unix_timestamp_to_ymdhms(ts);

        let mut buf: String<64> = String::new();
        let _ = write!(
            &mut buf,
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC",
            year,
            month,
            day,
            hour,
            minute,
            second
        );
        io.write_fmt(format_args!("{}\r\n", crate::ecma48::underline(buf.as_str())));
        return CommandAction::None;
    }

    if cmd.eq_ignore_ascii_case("insane") {
        let cols = (*term_cols).max(1);
        io.write_str("insane: iterating U+0000..=U+10FFFF (Ctrl-C to abort)\r\n");

        let mut col: usize = 0;
        for cp in 0u32..=0x10FFFF {
            if (cp & 0x3FF) == 0 {
                if let Some(b) = io.read_byte() {
                    if b == 0x03 {
                        io.write_str("\r\ninsane: aborted\r\n");
                        return CommandAction::None;
                    }
                }
            }

            let ch = match core::char::from_u32(cp) {
                Some(ch) if !ch.is_control() => ch,
                Some(_) => '.',
                    None => '\0',
            };

            io.write_char(ch);

            col += 1;
            if col >= cols {
                io.write_str("\r\n");
                col = 0;
            }
        }

        if col != 0 {
            io.write_str("\r\n");
        }
        io.write_str("insane: done\r\n");
        return CommandAction::None;
    }

    if cmd.eq_ignore_ascii_case("up") {
        io.write_str("line1\r\nline2\r\n");
        io.write_fmt(format_args!("{}", crate::ecma48::up(1)));
        io.write_str("↑\r\n");
        return CommandAction::None;
    }

    if let Some(rest) = cmd.strip_prefix("idle") {
        let rest = rest.trim();
        if rest.is_empty() {
            io.write_fmt(format_args!(
                "idle: {}\r\n",
                crate::power::idle_policy().as_str()
            ));
            return CommandAction::None;
        }
        let policy = match rest {
            "spin" => crate::power::IdlePolicy::Spin,
            "hlt" => crate::power::IdlePolicy::Halt,
            _ => {
                io.write_str("idle: usage idle [spin|hlt]\r\n");
                return CommandAction::None;
            }
        };
        let prev = crate::power::set_idle_policy(policy);
        io.write_fmt(format_args!(
            "idle: {} -> {}\r\n",
            prev.as_str(),
            policy.as_str()
        ));
        return CommandAction::None;
    }

    if let Some(rest) = cmd.strip_prefix("pstate") {
        let rest = rest.trim();
        if rest.is_empty() {
            let cur = crate::power::current_ratio();
            let armed = crate::power::msr_armed();
            let details = crate::power::msr_details().copied();

            match (cur, armed, details) {
                (Some(cur), true, Some(d)) => io.write_fmt(format_args!(
                    "pstate: current={} min={} max={}\r\n",
                    cur,
                    d.min_ratio.unwrap_or(0),
                    d.max_ratio.unwrap_or(0)
                )),
                (_, false, _) => io.write_str("pstate: msr disarmed\r\n"),
                (_, true, None) => io.write_str("pstate: msr details not probed\r\n"),
                _ => io.write_str("pstate: unsupported\r\n"),
            }
            return CommandAction::None;
        }

        let Some(req) = rest.parse::<u8>().ok() else {
            io.write_str("pstate: usage pstate <ratio>\r\n");
            return CommandAction::None;
        };

        match crate::power::set_pstate_ratio(req) {
            Ok(applied) => io.write_fmt(format_args!(
                "pstate: applied {}\r\n",
                applied
            )),
            Err(err) => io.write_fmt(format_args!(
                "pstate: failed: {}\r\n",
                err
            )),
        }
        return CommandAction::None;
    }

    if let Some(rest) = cmd.strip_prefix("echo ") {
        io.write_str(rest);
        io.write_str("\r\n");
        return CommandAction::None;
    }

    if cmd.eq_ignore_ascii_case("cube") {
        return CommandAction::EnterCube;
    }

    if cmd.eq_ignore_ascii_case("ico") {
        return CommandAction::EnterIco;
    }

    if cmd.eq_ignore_ascii_case("txt") || cmd.eq_ignore_ascii_case("txtedt") {
        let mut filename: String<48> = String::new();
        let _ = filename.push_str("untitled.txt");
        return CommandAction::EnterTxtEdt { filename };
    }

    io.write_str("unknown: ");
    io.write_str(cmd);
    io.write_str("\r\n");
    CommandAction::None
}

fn parse_set_dims(cmd: &str) -> Option<(usize, usize)> {
    let cmd = cmd.trim();
    let inner = cmd.strip_prefix("set(")?.strip_suffix(')')?;
    let (a, b) = inner.split_once(',')?;
    let cols = a.trim().parse::<usize>().ok()?;
    let rows = b.trim().parse::<usize>().ok()?;
    Some((cols, rows))
}

fn write_usize(io: &dyn ShellIo, value: usize) {
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    let mut v = value;
    if v == 0 {
        io.write_byte(b'0');
        return;
    }
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    for b in &buf[i..] {
        io.write_byte(*b);
    }
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

fn draw_corners(io: &dyn ShellIo, cols: usize, rows: usize) {
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
    crate::shellcube::enter_mode();
}

pub(crate) mod uart1_com1 {
    use core::fmt;
    use core::sync::atomic::{AtomicBool, Ordering};

    const COM1: u16 = 0x3F8;
    static INIT: AtomicBool = AtomicBool::new(false);

    pub(crate) fn init() {
        if INIT.swap(true, Ordering::AcqRel) {
            return;
        }
        unsafe {
            crate::portio::outb(COM1 + 1, 0x00); // disable IRQs
            crate::portio::outb(COM1 + 3, 0x80); // DLAB on
            crate::portio::outb(COM1 + 0, 0x01); // divisor low (115200)
            crate::portio::outb(COM1 + 1, 0x00); // divisor high
            crate::portio::outb(COM1 + 3, 0x03); // 8N1
            crate::portio::outb(COM1 + 2, 0xC7); // FIFO enable
            crate::portio::outb(COM1 + 4, 0x0B); // IRQs, RTS/DSR
        }
    }

    #[inline]
    pub(crate) fn write_byte(b: u8) {
        if !INIT.load(Ordering::Acquire) {
            init();
        }
        unsafe {
            while (crate::portio::inb(COM1 + 5) & 0x20) == 0 {}
            crate::portio::outb(COM1, b);
        }
    }

    pub(crate) fn write_str(s: &str) {
        for &b in s.as_bytes() {
            if b == b'\n' {
                write_byte(b'\r');
            }
            write_byte(b);
        }
    }

    pub(crate) fn write_bytes(bytes: &[u8]) {
        for &b in bytes {
            write_byte(b);
        }
    }

    pub(crate) fn write_fmt(args: fmt::Arguments<'_>) {
        use core::fmt::Write;

        struct Writer;

        impl fmt::Write for Writer {
            fn write_str(&mut self, s: &str) -> fmt::Result {
                for &b in s.as_bytes() {
                    if b == b'\n' {
                        write_byte(b'\r');
                    }
                    write_byte(b);
                }
                Ok(())
            }
        }

        let _ = Writer.write_fmt(args);
    }

    pub(crate) fn write_char(ch: char) {
        let mut buf = [0u8; 4];
        let s = ch.encode_utf8(&mut buf);
        write_str(s);
    }

    pub(crate) fn read_byte() -> Option<u8> {
        if !INIT.load(Ordering::Acquire) {
            init();
        }
        unsafe {
            if (crate::portio::inb(COM1 + 5) & 0x01) != 0 {
                Some(crate::portio::inb(COM1))
            } else {
                None
            }
        }
    }
}
