use core::str::SplitWhitespace;

use super::super::{ShellBackend2, print_shell_line};
use super::tlb_helper::print_table;
use crate::shell2::shell2_cmd::ParseOutcome;

const HYPER_MENU_HEADERS: [&str; 2] = ["Subcommand", "Description"];
const HYPER_MENU_ROWS: [[&str; 2]; 2] = [
    ["status", "Show the kernel Hyper transport surfaces"],
    ["probe", "Describe the background HTTP/1 probe service"],
];

fn line(io: &'static dyn ShellBackend2, text: &str) {
    print_shell_line(io, text);
}

fn print_status(io: &'static dyn ShellBackend2) {
    line(io, "hyper: client=http1 transport=tokio/vnet");
    line(io, "hyper: http fetch=body+stream-to-trueosfs");
    line(io, "hyper: https fetch=rustls body+stream-to-trueosfs");
    line(io, "hyper: probe=spawn-svc hyper-http1-probe");
}

fn print_probe(io: &'static dyn ShellBackend2) {
    line(io, "hyper probe: boot loopback validates HTTP/1 client");
    line(io, "hyper probe: background net probe waits for socket+gateway readiness");
    line(io, "hyper probe: target example.de:80 GET /");
}

fn print_usage(io: &'static dyn ShellBackend2) {
    print_table(io, &HYPER_MENU_HEADERS, &HYPER_MENU_ROWS);
}

pub(crate) fn try_parse(
    io: &'static dyn ShellBackend2,
    args: &mut SplitWhitespace<'_>,
) -> ParseOutcome {
    match args.next() {
        None | Some("status") => print_status(io),
        Some("probe") => print_probe(io),
        Some("help") | Some("-h") | Some("--help") => print_usage(io),
        Some(_) => print_usage(io),
    }

    ParseOutcome::Handled
}
