use core::str::SplitWhitespace;

use embassy_time::{Duration as EmbassyDuration, Timer};

use super::super::{ShellBackend2, print_shell_line};
use crate::shell2::shell2_cmd::ParseOutcome;

const GO_CHARS: [char; 9] = ['⣿', '⣾', '⣽', '⣻', '⢿', '⡿', '⣟', '⣯', '⣷'];
const GO_TWO_CHARS: [char; 9] = ['⢈', '⡈', '⡐', '⡠', '⣀', '⢄', '⢂', '⢁', '⡁'];
const INSANE_MAX_CP: u32 = 0x87FFF;
const DEFAULT_ECMA_COLS: usize = 100;

fn start_looping_chars(io: &'static dyn ShellBackend2, label: &str, chars: &'static [char]) {
    let started = alloc::format!("etc: {} started (50ms loop)", label);
    print_shell_line(io, started.as_str());

    crate::wait::spawn_local_detached(async move {
        let mut idx = 0usize;
        loop {
            io.write_char(chars[idx]);
            idx = (idx + 1) % chars.len();
            Timer::after(EmbassyDuration::from_millis(50)).await;
        }
    });
}

fn cmd_insane(io: &'static dyn ShellBackend2) {
    io.write_str("insane: iterating U+0000..=U+087FFF (Ctrl-C to abort)\r\n");

    let mut col = 0usize;
    for cp in 0u32..=INSANE_MAX_CP {
        if (cp & 0x3FF) == 0
            && let Some(b) = io.read_byte()
            && b == 0x03
        {
            io.write_str("\r\ninsane: aborted\r\n");
            return;
        }

        let ch = match core::char::from_u32(cp) {
            Some(ch) if !ch.is_control() => ch,
            Some(_) => '.',
            None => '\u{FFFD}',
        };

        io.write_char(ch);
        col += 1;
        if col >= DEFAULT_ECMA_COLS {
            io.write_str("\r\n");
            col = 0;
        }
    }

    if col != 0 {
        io.write_str("\r\n");
    }
    io.write_str("insane: done\r\n");
}

fn print_usage(io: &'static dyn ShellBackend2) {
    print_shell_line(
        io,
        "etc: usage `etc go|go2|insane|ecma [demo|sanitize <text>|clear|help]`",
    );
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
        "go" => start_looping_chars(io, "go", &GO_CHARS),
        "go2" => start_looping_chars(io, "go2", &GO_TWO_CHARS),
        "insane" => cmd_insane(io),
        "ecma" => {
            let rest = args.collect::<alloc::vec::Vec<_>>().join(" ");
            super::super::ecma48::demo_ecma48(io, rest.as_str(), DEFAULT_ECMA_COLS);
        }
        _ => print_usage(io),
    }

    ParseOutcome::Handled
}
