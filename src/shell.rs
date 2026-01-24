use core::fmt::Write;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use heapless::String;

use crate::shellcube::{CubeState, WireShape, CUBE_COLS, CUBE_ROWS};
use crate::ecma48;
use crate::disc::{block, install};

const PROMPT_RGB: (u8, u8, u8) = (255, 55, 255);

static NEXT_JOB_ID: AtomicUsize = AtomicUsize::new(1);

#[inline]
fn write_prompt() {
    uart1_com1::write_fmt(format_args!("{}", crate::ecma48::color("§ ", PROMPT_RGB)));
}

#[inline]
fn write_banner() {
    uart1_com1::write_fmt(format_args!("{}\n", crate::ecma48::bold("TRUE OS")));
    write_prompt();
}

static TERM_COLS: AtomicUsize = AtomicUsize::new(80);
static TERM_ROWS: AtomicUsize = AtomicUsize::new(24);
static GO_MODE: AtomicBool = AtomicBool::new(false);
const GO_CHARS: [char; 9] = ['⣿', '⣾', '⣽', '⣻', '⢿', '⡿', '⣟', '⣯', '⣷'];

#[inline]
fn set_go_mode(enable: bool) {
    let prev = GO_MODE.swap(enable, Ordering::Release);
    if enable && !prev {
        uart1_com1::write_str(ecma48::HIDE_CURSOR);
    } else if !enable && prev {
        uart1_com1::write_str(ecma48::SHOW_CURSOR);
    }
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
}

#[embassy_executor::task]
pub async fn task(spawner: Spawner) {
    uart1_com1::init();

    write_banner();

    let mut line: String<128> = String::new();
    let mut go_idx: usize = 0;
    let mut pending_action: Option<PendingAction> = None;
    let mut pending_deadline: Option<Instant> = None;
    let mut cube_mode = true;
    let mut cube = CubeState::new();
    cube.set_shape(WireShape::Cube);
    cube.reset();
    enter_cube_mode();

    loop {
        if let Some(b) = uart1_com1::read_byte() {
            if cube_mode {
                if b == b'\r' || b == b'\n' {
                    cube_mode = false;
                    set_go_mode(false);
                    uart1_com1::write_str(ecma48::CLEAR_SCREEN);
                    uart1_com1::write_str(ecma48::HOME);
                    write_banner();
                }
                continue;
            }
            match b {
                b'\r' | b'\n' | b' ' if pending_action.is_some() => {
                    // Pending install confirmation: Enter = proceed, Space = abort.
                    if let Some(PendingAction::Install { raw_id, migrate }) = pending_action {
                        match b {
                            b'\r' | b'\n' => {
                                pending_action = None;
                                pending_deadline = None;
                                set_go_mode(false);
                                line.clear();
                                uart1_com1::write_str("\r\n");
                                do_install(raw_id, migrate);
                                write_prompt();
                            }
                            b' ' => {
                                pending_action = None;
                                pending_deadline = None;
                                set_go_mode(false);
                                line.clear();
                                uart1_com1::write_str("\r\ninstall: aborted\r\n");
                                write_prompt();
                            }
                            _ => {}
                        }
                        continue;
                    }

                    // Other pending actions: Enter/Space cancels.
                    pending_action = None;
                    pending_deadline = None;
                    set_go_mode(false);
                    line.clear();
                    uart1_com1::write_str("\r\n");
                    write_prompt();
                    continue;
                }
                b'\r' | b'\n' => {
                    if line.is_empty() && pending_action.is_none() && GO_MODE.load(Ordering::Acquire) {
                        set_go_mode(false);
                        uart1_com1::write_str("\r\n");
                        write_prompt();
                        continue;
                    }
                    if !line.is_empty() {
                        uart1_com1::write_str("\r\n");
                        let action = handle_line(&line, &spawner);
                        line.clear();
                        // During multiline Porth capture, avoid spamming the prompt each line.
                        if !crate::porth::shell_is_capturing_multiline() {
                            write_prompt();
                        }
                        match action {
                            CommandAction::Pending(action) => {
                                pending_action = Some(action);
                                pending_deadline = match action {
                                    PendingAction::Reset | PendingAction::S5 => {
                                        Some(Instant::now() + EmbassyDuration::from_secs(5))
                                    }
                                    PendingAction::Install { .. } => None,
                                };
                                set_go_mode(matches!(action, PendingAction::Reset | PendingAction::S5));
                            }
                            CommandAction::EnterCube => {
                                cube_mode = true;
                                set_go_mode(false);
                                cube.set_shape(WireShape::Cube);
                                cube.reset();
                                enter_cube_mode();
                            }
                            CommandAction::EnterIco => {
                                cube_mode = true;
                                set_go_mode(false);
                                cube.set_shape(WireShape::Icosidodecahedron);
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
                    // Delegate Ctrl-C handling to Porth (compile abort / repl exit) if active.
                    if !crate::porth::shell_handle_ctrl_c() {
                        uart1_com1::write_str("^C\r\n");
                    }
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
                Timer::after(EmbassyDuration::from_millis(333)).await;
                continue;
            }
            if let (Some(action), Some(deadline)) = (pending_action, pending_deadline) {
                if Instant::now() >= deadline {
                    set_go_mode(false);
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
                        PendingAction::Install { .. } => {}
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

fn handle_line(line: &str, spawner: &Spawner) -> CommandAction {
    let cmd = line.trim();
    if cmd.is_empty() {
        return CommandAction::None;
    }

    // Background job operator: `§ <command...>`
    // Runs the command asynchronously and prints a completion marker when done.
    if let Some(rest) = cmd.strip_prefix('§') {
        let rest = rest.trim_start();
        if rest.is_empty() {
            uart1_com1::write_str("usage: § <command...>\r\n");
            uart1_com1::write_str("note: currently supports Porth commands\r\n");
            return CommandAction::None;
        }
        spawn_background_job(spawner, rest);
        return CommandAction::None;
    }

    // Delegate Porth-related commands + modes to the Porth module.
    if crate::porth::shell_handle_line(cmd) {
        return CommandAction::None;
    }

    if let Some((verb, rest)) = cmd.split_once(' ') {
        if verb.eq_ignore_ascii_case("install") {
            return handle_install(rest);
        }
    } else if cmd.eq_ignore_ascii_case("install") {
        return handle_install("");
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
        set_go_mode(true);
        return CommandAction::None;
    }

    if cmd.eq_ignore_ascii_case("mandel") {
        crate::draw_mandelbrot();
        uart1_com1::write_str("mandel ok\r\n");
        return CommandAction::None;
    }

    if cmd.eq_ignore_ascii_case("time") {
        let Some(boot_ts) = crate::limine::boot_timestamp_secs() else {
            uart1_com1::write_str("time: boot timestamp unavailable\r\n");
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
        uart1_com1::write_fmt(format_args!("{}\r\n", crate::ecma48::underline(buf.as_str())));
        return CommandAction::None;
    }

    if cmd.eq_ignore_ascii_case("up") {
        uart1_com1::write_str("line1\r\nline2\r\n");
        uart1_com1::write_fmt(format_args!("{}", crate::ecma48::up(1)));
        uart1_com1::write_str("↑\r\n");
        return CommandAction::None;
    }

    if let Some(rest) = cmd.strip_prefix("idle") {
        let rest = rest.trim();
        if rest.is_empty() {
            uart1_com1::write_fmt(format_args!(
                "idle: {}\r\n",
                crate::power::idle_policy().as_str()
            ));
            return CommandAction::None;
        }
        let policy = match rest {
            "spin" => crate::power::IdlePolicy::Spin,
            "hlt" => crate::power::IdlePolicy::Halt,
            _ => {
                uart1_com1::write_str("idle: usage idle [spin|hlt]\r\n");
                return CommandAction::None;
            }
        };
        let prev = crate::power::set_idle_policy(policy);
        uart1_com1::write_fmt(format_args!(
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
            let caps = crate::power::caps().copied();
            match (cur, caps) {
                (Some(cur), Some(caps)) => {
                    uart1_com1::write_fmt(format_args!(
                        "pstate: current={} min={} max={}\r\n",
                        cur,
                        caps.min_ratio.unwrap_or(0),
                        caps.max_ratio.unwrap_or(0)
                    ));
                }
                _ => uart1_com1::write_str("pstate: unsupported\r\n"),
            }
            return CommandAction::None;
        }

        let Some(req) = rest.parse::<u8>().ok() else {
            uart1_com1::write_str("pstate: usage pstate <ratio>\r\n");
            return CommandAction::None;
        };

        match crate::power::set_pstate_ratio(req) {
            Ok(applied) => uart1_com1::write_fmt(format_args!(
                "pstate: applied {}\r\n",
                applied
            )),
            Err(err) => uart1_com1::write_fmt(format_args!(
                "pstate: failed: {}\r\n",
                err
            )),
        }
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

    if cmd.eq_ignore_ascii_case("ico") {
        return CommandAction::EnterIco;
    }

    uart1_com1::write_str("unknown: ");
    uart1_com1::write_str(cmd);
    uart1_com1::write_str("\r\n");
    CommandAction::None
}

fn spawn_background_job(spawner: &Spawner, cmd: &str) {
    let mut buf: heapless::String<256> = heapless::String::new();
    if buf.push_str(cmd).is_err() {
        uart1_com1::write_str("§: command too long\r\n");
        return;
    }

    let id = NEXT_JOB_ID.fetch_add(1, Ordering::Relaxed);
    if spawner.spawn(shell_bg_job(id, buf)).is_err() {
        uart1_com1::write_str("§: spawn failed\r\n");
    }
}

#[embassy_executor::task(pool_size = 4)]
async fn shell_bg_job(id: usize, cmd: heapless::String<256>) {
    uart1_com1::write_fmt(format_args!("\r\n§{} start: {}\r\n", id, cmd.as_str()));

    // For now, only Porth commands are supported in the background.
    if !crate::porth::shell_handle_line(cmd.as_str()) {
        uart1_com1::write_fmt(format_args!("§{}: unsupported command\r\n", id));
    }

    uart1_com1::write_fmt(format_args!("§{} done\r\n", id));
}

fn handle_install(args: &str) -> CommandAction {
    let args = args.trim();

    if args.is_empty() {
        uart1_com1::write_str("install: BIOS/MBR install (DESTRUCTIVE)\r\n");
        uart1_com1::write_str("usage: install <disc_id>\r\n");
        uart1_com1::write_str("       install <disc_id> migrate\r\n");
        uart1_com1::write_str("example: install 1\r\n");
        uart1_com1::write_str("example: install 1 migrate\r\n");
        uart1_com1::write_str("available disks:\r\n");
        for h in block::device_handles().into_iter() {
            let info = h.info();
            uart1_com1::write_fmt(format_args!(
                "  id={} ({}) kind={:?} blocks={} bs={} writable={} label={:?}\r\n",
                info.id.raw(),
                info.id,
                info.kind,
                info.block_count,
                info.block_size,
                info.writable,
                info.label
            ));
        }
        return CommandAction::None;
    }

    let mut parts = args.split_whitespace();
    let first = match parts.next() {
        Some(s) => s,
        None => {
            uart1_com1::write_str("install: missing args\r\n");
            return CommandAction::None;
        }
    };

    // Supported forms:
    //   install <id>
    //   install <id> migrate
    //   install migrate <id>
    let (mode_migrate, id_str) = if first.eq_ignore_ascii_case("migrate") {
        let id_str = match parts.next() {
            Some(s) => s,
            None => {
                uart1_com1::write_str("install: missing id\r\n");
                return CommandAction::None;
            }
        };
        (true, id_str)
    } else {
        let second = parts.next();
        match second {
            Some(s2) if s2.eq_ignore_ascii_case("migrate") => (true, first),
            Some(_) => (false, first),
            None => (false, first),
        }
    };

    let raw_id = match parse_disc_id_raw(id_str) {
        Some(v) => v,
        None => {
            uart1_com1::write_str("install: invalid id (use decimal like '1' or 'disc001')\r\n");
            return CommandAction::None;
        }
    };

    let target = block::device_handles().into_iter().find(|h| h.id().raw() == raw_id);
    let Some(handle) = target else {
        uart1_com1::write_str("install: no such device\r\n");
        return CommandAction::None;
    };

    let info = handle.info();
    uart1_com1::write_fmt(format_args!(
        "install: target id={} ({}) label={:?} blocks={} bs={}\r\n",
        info.id.raw(),
        info.id,
        info.label,
        info.block_count,
        info.block_size
    ));
    if mode_migrate {
        uart1_com1::write_str(
            "install: migrate shifts a FAT superfloppy (FAT-at-LBA0) forward by 1MiB into a partition.\r\n",
        );
        uart1_com1::write_str(
            "install: it validates the last 1MiB is free; otherwise it aborts to avoid data loss.\r\n",
        );
        uart1_com1::write_str("install: still destructive; always back up.\r\n");
    } else {
        uart1_com1::write_str("install: this will ERASE the disk.\r\n");
    }
    uart1_com1::write_str("install: press Enter to proceed, Space to abort\r\n");

    CommandAction::Pending(PendingAction::Install {
        raw_id: info.id.raw(),
        migrate: mode_migrate,
    })
}

fn do_install(raw_id: u32, mode_migrate: bool) {
    let target = block::device_handles().into_iter().find(|h| h.id().raw() == raw_id);
    let Some(handle) = target else {
        uart1_com1::write_str("install: no such device\r\n");
        return;
    };

    let info = handle.info();
    uart1_com1::write_fmt(format_args!(
        "install: installing Limine BIOS + TRUEOS to id={} ({})...\r\n",
        info.id.raw(),
        info.id
    ));

    set_go_mode(true);
    let mut go_idx: usize = 0;
    let mut tick = || {
        let ch = GO_CHARS[go_idx];
        go_idx = (go_idx + 1) % GO_CHARS.len();
        uart1_com1::write_str("\r");
        write_prompt();
        uart1_com1::write_char(ch);
    };

    let mut status = |args: core::fmt::Arguments<'_>| {
        // Print a user-visible status line without fighting the spinner.
        uart1_com1::write_str("\r\n");
        uart1_com1::write_fmt(args);
        uart1_com1::write_str("\r\n");
    };

    let res = if mode_migrate {
        install::install_bios_mbr_migrate_superfloppy_with_progress_and_status(handle, &mut tick, &mut status)
    } else {
        install::install_bios_mbr_with_progress(handle, &mut tick)
    };
    set_go_mode(false);
    uart1_com1::write_str("\r\n");

    match res {
        Ok(()) => uart1_com1::write_str("install: ok\r\n"),
        Err(e) => uart1_com1::write_fmt(format_args!("install: failed: {:?}\r\n", e)),
    }
}

fn parse_disc_id_raw(s: &str) -> Option<u32> {
    let s = s.trim();
    let s = s.strip_prefix("disc").unwrap_or(s);
    s.parse::<u32>().ok()
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
