use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use heapless::String;

use crate::shellcube::{CubeState, CUBE_COLS, CUBE_ROWS};
use crate::ecma48;

const PROMPT_RGB: (u8, u8, u8) = (255, 55, 255);

#[inline]
fn write_prompt() {
    uart1_com1::write_fmt(format_args!("{}", crate::ecma48::color("§ ", PROMPT_RGB)));
}

#[inline]
fn write_banner() {
    uart1_com1::write_str("TRUE OS\n");
    write_prompt();
}

static TERM_COLS: AtomicUsize = AtomicUsize::new(80);
static TERM_ROWS: AtomicUsize = AtomicUsize::new(24);
static GO_MODE: AtomicBool = AtomicBool::new(false);
const GO_CHARS: [char; 9] = ['⣿', '⣾', '⣽', '⣻', '⢿', '⡿', '⣟', '⣯', '⣷'];

#[derive(Copy, Clone)]
enum PendingAction {
    Reset,
    S5,
}

enum CommandAction {
    None,
    Pending(PendingAction),
    EnterCube,
}

#[embassy_executor::task]
pub async fn task() {
    uart1_com1::init();

    write_banner();

    let mut line: String<128> = String::new();
    let mut go_idx: usize = 0;
    let mut pending_action: Option<PendingAction> = None;
    let mut pending_deadline: Option<Instant> = None;
    let mut cube_mode = true;
    let mut cube = CubeState::new();
    cube.reset();
    enter_cube_mode();

    loop {
        if let Some(b) = uart1_com1::read_byte() {
            if cube_mode {
                if b == b'\r' || b == b'\n' {
                    cube_mode = false;
                    GO_MODE.store(false, Ordering::Release);
                    uart1_com1::write_str(ecma48::CLEAR_SCREEN);
                    uart1_com1::write_str(ecma48::HOME);
                    write_banner();
                }
                continue;
            }
            match b {
                b'\r' | b'\n' => {
                    if pending_action.is_some() {
                        pending_action = None;
                        pending_deadline = None;
                        GO_MODE.store(false, Ordering::Release);
                        line.clear();
                        uart1_com1::write_str("\r\n");
                        write_prompt();
                        continue;
                    }
                    if !line.is_empty() {
                        uart1_com1::write_str("\r\n");
                        let action = handle_line(&line);
                        line.clear();
                        write_prompt();
                        match action {
                            CommandAction::Pending(action) => {
                                pending_action = Some(action);
                                pending_deadline =
                                    Some(Instant::now() + EmbassyDuration::from_secs(5));
                                GO_MODE.store(true, Ordering::Release);
                            }
                            CommandAction::EnterCube => {
                                cube_mode = true;
                                GO_MODE.store(false, Ordering::Release);
                                cube.reset();
                                enter_cube_mode();
                            }
                            CommandAction::None => {}
                        }
                    }
                }
                0x08 | 0x7F => {
                    if !line.is_empty() {
                        line.pop();
                        uart1_com1::write_str("\x08 \x08");
                    }
                }
                0x03 => {
                    line.clear();
                    uart1_com1::write_str("^C\r\n");
                    write_prompt();
                }
                _ => {
                    if b >= 0x20 {
                        if line.push(b as char).is_ok() {
                            uart1_com1::write_byte(b);
                        }
                    }
                }
            }
        } else {
            if cube_mode {
                cube.draw_frame();
                Timer::after(EmbassyDuration::from_millis(200)).await;
                continue;
            }
            if let (Some(action), Some(deadline)) = (pending_action, pending_deadline) {
                if Instant::now() >= deadline {
                    GO_MODE.store(false, Ordering::Release);
                    pending_action = None;
                    pending_deadline = None;
                    match action {
                        PendingAction::Reset => {
                            if let Err(err) = crate::acpi::facp::reset_system() {
                                uart1_com1::write_str("tlb miss warn\r\n");
                                write_prompt();
                            }
                        }
                        PendingAction::S5 => {
                            if crate::acpi::facp::enter_s5(0, None).is_err() {
                                uart1_com1::write_str("\r\ns5 failed\r\n");
                                write_prompt();
                            }
                        }
                    }
                    continue;
                }
            }
            if GO_MODE.load(Ordering::Acquire) {
                let ch = GO_CHARS[go_idx];
                go_idx = (go_idx + 1) % GO_CHARS.len();
                uart1_com1::write_str("\r");
                write_prompt();
                uart1_com1::write_char(ch);
                Timer::after(EmbassyDuration::from_millis(160)).await;
            } else {
                Timer::after(EmbassyDuration::from_millis(2)).await;
            }
        }
    }
}

fn handle_line(line: &str) -> CommandAction {
    let cmd = line.trim();
    if cmd.is_empty() {
        return CommandAction::None;
    }

    if let Some((cols, rows)) = parse_set_dims(cmd) {
        TERM_COLS.store(cols, Ordering::Release);
        TERM_ROWS.store(rows, Ordering::Release);
        uart1_com1::write_str("term set: ");
        write_usize(cols);
        uart1_com1::write_str("x");
        write_usize(rows);
        uart1_com1::write_str("\r\n");
        draw_corners(cols, rows);
        return CommandAction::None;
    }

    if cmd.eq_ignore_ascii_case("reset") {
        return CommandAction::Pending(PendingAction::Reset);
    }

    if cmd.eq_ignore_ascii_case("s5") {
        return CommandAction::Pending(PendingAction::S5);
    }

    if cmd.eq_ignore_ascii_case("go") {
        GO_MODE.store(true, Ordering::Release);
        return CommandAction::None;
    }

    if cmd.eq_ignore_ascii_case("mandel") {
        crate::draw_mandelbrot();
        uart1_com1::write_str("mandel ok\r\n");
        return CommandAction::None;
    }

    if let Some(rest) = cmd.strip_prefix("echo ") {
        uart1_com1::write_str(rest);
        uart1_com1::write_str("\r\n");
        return CommandAction::None;
    }

    if cmd.eq_ignore_ascii_case("cube") {
        return CommandAction::EnterCube;
    }

    uart1_com1::write_str("unknown: ");
    uart1_com1::write_str(cmd);
    uart1_com1::write_str("\r\n");
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

fn write_usize(value: usize) {
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    let mut v = value;
    if v == 0 {
        uart1_com1::write_byte(b'0');
        return;
    }
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    for b in &buf[i..] {
        uart1_com1::write_byte(*b);
    }
}

fn draw_corners(cols: usize, rows: usize) {
    if cols == 0 || rows == 0 {
        return;
    }
    uart1_com1::write_str(crate::ecma48::SAVE_CURSOR);
    // top-right
    write_pos(1, cols);
    uart1_com1::write_byte(b'O');
    // bottom-left
    write_pos(rows, 1);
    uart1_com1::write_byte(b'O');
    // bottom-right
    write_pos(rows, cols);
    uart1_com1::write_byte(b'O');
    uart1_com1::write_str(crate::ecma48::RESTORE_CURSOR);
}

#[inline]
fn write_pos(row: usize, col: usize) {
    uart1_com1::write_fmt(format_args!("{}", crate::ecma48::pos(row, col)));
}

fn enter_cube_mode() {
    TERM_COLS.store(CUBE_COLS, Ordering::Release);
    TERM_ROWS.store(CUBE_ROWS, Ordering::Release);
    draw_corners(CUBE_COLS, CUBE_ROWS);
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
