use core::str::SplitWhitespace;
use core::sync::atomic::{AtomicU8, Ordering};

use embassy_time::{Duration as EmbassyDuration, Timer};

use super::super::{NET_TCP_SHELL_BACKEND, ShellBackend2, UART1_COM1_BACKEND, print_shell_line};
use super::tlb_helper::print_table;
use crate::shell2::ecma48;
use crate::shell2::shell2_cmd::ParseOutcome;

const GO_CHARS: [char; 9] = ['⣿', '⣾', '⣽', '⣻', '⢿', '⡿', '⣟', '⣯', '⣷'];
const GO_TWO_CHARS: [char; 9] = ['⢈', '⡈', '⡐', '⡠', '⣀', '⢄', '⢂', '⢁', '⡁'];
const INSANE_MAX_CP: u32 = 0x27FFF;
const DEFAULT_INSANE_COLS: usize = 100;
const BACKEND_UART_MASK: u8 = 1 << 0;
const BACKEND_NET_MASK: u8 = 1 << 1;
const ETC_MENU_HEADERS: [&str; 2] = ["Subcommand", "Description"];
const ETC_MENU_ROWS: [[&str; 2]; 4] = [
    ["ample", "Launch the text-mode Ample shell app"],
    ["go", "Loop spinner glyphs until interrupted"],
    ["go2", "Loop alternate spinner glyphs until interrupted"],
    ["insane", "Print a wide Unicode sweep"],
];

static GO_ACTIVE_MASK: AtomicU8 = AtomicU8::new(0);

fn same_backend(io: &'static dyn ShellBackend2, target: &'static dyn ShellBackend2) -> bool {
    (io as *const dyn ShellBackend2 as *const ())
        == (target as *const dyn ShellBackend2 as *const ())
}

fn backend_mask(io: &'static dyn ShellBackend2) -> u8 {
    let uart_backend: &'static dyn ShellBackend2 = &UART1_COM1_BACKEND;
    if same_backend(io, uart_backend) {
        return BACKEND_UART_MASK;
    }

    let net_backend: &'static dyn ShellBackend2 = &NET_TCP_SHELL_BACKEND;
    if same_backend(io, net_backend) {
        return BACKEND_NET_MASK;
    }

    0
}

pub(crate) fn handle_input_byte(io: &'static dyn ShellBackend2) -> bool {
    let mask = backend_mask(io);
    if mask == 0 {
        return false;
    }
    if GO_ACTIVE_MASK.fetch_and(!mask, Ordering::AcqRel) & mask == 0 {
        return false;
    }
    true
}

fn start_looping_chars(
    io: &'static dyn ShellBackend2,
    label: &'static str,
    chars: &'static [char],
) {
    let mask = backend_mask(io);
    let started = alloc::format!("etc: {} started (50ms loop)", label);
    print_shell_line(io, started.as_str());
    if mask != 0 {
        GO_ACTIVE_MASK.fetch_or(mask, Ordering::Release);
    }

    crate::wait::spawn_local_detached(async move {
        let mut idx = 0usize;
        loop {
            if mask != 0 && (GO_ACTIVE_MASK.load(Ordering::Acquire) & mask) == 0 {
                io.raw_write_str("\x08 \x08");
                io.raw_write_str(ecma48::RESET);
                io.raw_write_str(ecma48::SHOW_CURSOR);
                io.raw_write_str(ecma48::CURSOR_STEADY_BLOCK);
                let stopped = alloc::format!("etc: {} stopped", label);
                print_shell_line(io, stopped.as_str());
                break;
            }
            io.raw_write_char(chars[idx]);
            io.raw_write_str("\x08");
            idx = (idx + 1) % chars.len();
            Timer::after(EmbassyDuration::from_millis(50)).await;
        }
    });
}

fn cmd_insane(io: &'static dyn ShellBackend2) {
    io.raw_write_str("insane: iterating U+0000..=U+087FFF (Ctrl-C to abort)\r\n");

    let mut col = 0usize;
    for cp in 0u32..=INSANE_MAX_CP {
        if (cp & 0x3FF) == 0
            && let Some(b) = io.read_byte()
            && b == 0x03
        {
            io.raw_write_str("\r\ninsane: aborted\r\n");
            return;
        }

        let ch = match core::char::from_u32(cp) {
            Some(ch) if !ch.is_control() => ch,
            Some(_) => '.',
            None => '\u{FFFD}',
        };

        io.raw_write_char(ch);
        col += 1;
        if col >= DEFAULT_INSANE_COLS {
            io.raw_write_str("\r\n");
            col = 0;
        }
    }

    if col != 0 {
        io.raw_write_str("\r\n");
    }
    io.raw_write_str("insane: done\r\n");
}

fn print_usage(io: &'static dyn ShellBackend2) {
    print_table(io, &ETC_MENU_HEADERS, &ETC_MENU_ROWS);
}

pub(crate) fn try_parse(
    io: &'static dyn ShellBackend2,
    args: &mut SplitWhitespace<'_>,
) -> ParseOutcome {
    let Some(cmd) = args.next() else {
        print_usage(io);
        return ParseOutcome::Handled;
    };

    match cmd {
        "ample" => return super::ample::try_parse(io, args),
        "go" => start_looping_chars(io, "go", &GO_CHARS),
        "go2" => start_looping_chars(io, "go2", &GO_TWO_CHARS),
        "insane" => cmd_insane(io),
        _ => print_usage(io),
    }

    ParseOutcome::Handled
}
