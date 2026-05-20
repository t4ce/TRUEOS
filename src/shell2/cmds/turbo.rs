use core::str::SplitWhitespace;

use super::super::{print_shell_line, ShellBackend2};
use super::tlb_helper::print_table;
use crate::power::turbo as power_turbo;
use crate::shell2::shell2_cmd::ParseOutcome;

const TURBO_MENU_HEADERS: [&str; 2] = ["Subcommand", "Arguments"];
const TURBO_MENU_ROWS: [[&str; 2]; 4] = [
    ["status", ""],
    ["on", ""],
    ["off", ""],
    ["verify", "[spins]"],
];

fn line(io: &'static dyn ShellBackend2, text: &str) {
    print_shell_line(io, text);
}

fn print_usage(io: &'static dyn ShellBackend2) {
    print_table(io, &TURBO_MENU_HEADERS, &TURBO_MENU_ROWS);
}

pub(crate) fn try_parse(
    io: &'static dyn ShellBackend2,
    args: &mut SplitWhitespace<'_>,
) -> ParseOutcome {
    let op = args.next().unwrap_or("status").trim();

    if op.is_empty() || op.eq_ignore_ascii_case("status") {
        if args.next().is_some() {
            print_usage(io);
            return ParseOutcome::Handled;
        }

        match power_turbo::local_state() {
            Ok(state) => {
                let msg = alloc::format!("turbo: state={:?}", state);
                line(io, msg.as_str());
            }
            Err(power_turbo::TurboSetError::Unsupported) => {
                line(io, "turbo: unsupported (intel-only)");
            }
            Err(power_turbo::TurboSetError::Disarmed) => {
                line(io, "turbo: disarmed");
            }
        }
        return ParseOutcome::Handled;
    }

    if op.eq_ignore_ascii_case("verify") {
        let spins = match args.next() {
            Some(raw) => match raw.parse::<usize>() {
                Ok(value) => value,
                Err(_) => {
                    print_usage(io);
                    return ParseOutcome::Handled;
                }
            },
            None => 200_000,
        };
        if args.next().is_some() {
            print_usage(io);
            return ParseOutcome::Handled;
        }

        match power_turbo::verify_all(spins) {
            Ok(report) => {
                let suffix = if report.timed_out { " TIMEOUT" } else { "" };
                let msg = alloc::format!(
                    "turbo: verify spins={} turbo={} noturbo={} unknown={} completed_aps={}/{} online_aps={} busy={} total_cpus={} seq={}{}",
                    spins,
                    report.turbo_cpus,
                    report.noturbo_cpus,
                    report.unknown_cpus,
                    report.completed_aps,
                    report.submitted_aps,
                    report.online_aps,
                    report.busy_aps,
                    report.total_cpus,
                    report.seq,
                    suffix
                );
                line(io, msg.as_str());
            }
            Err(power_turbo::TurboSetError::Disarmed) => {
                line(io, "turbo: verify disarmed unexpectedly");
            }
            Err(power_turbo::TurboSetError::Unsupported) => {
                line(io, "turbo: unsupported (intel-only)");
            }
        }

        return ParseOutcome::Handled;
    }

    let enable = if op.eq_ignore_ascii_case("on") {
        Some(true)
    } else if op.eq_ignore_ascii_case("off") {
        Some(false)
    } else {
        None
    };

    let Some(enable) = enable else {
        print_usage(io);
        return ParseOutcome::Handled;
    };
    if args.next().is_some() {
        print_usage(io);
        return ParseOutcome::Handled;
    }

    match power_turbo::set_enabled_all(enable) {
        Ok(report) => {
            let msg = alloc::format!(
                "turbo: requested={} ap_submitted={}/{} busy={} total_cpus={} seq={}",
                if report.requested_enable { "on" } else { "off" },
                report.submitted_aps,
                report.targeted_aps,
                report.busy_aps,
                report.total_cpus,
                report.seq
            );
            line(io, msg.as_str());
        }
        Err(power_turbo::TurboSetError::Disarmed) => {
            line(io, "turbo: write gate is disarmed");
        }
        Err(power_turbo::TurboSetError::Unsupported) => {
            line(io, "turbo: unsupported (intel-only)");
        }
    }

    ParseOutcome::Handled
}
