use core::str::SplitWhitespace;

use super::super::{ShellBackend2, print_shell_line};
use crate::shell2::shell2_cmd::ParseOutcome;

enum AcpiAction {
    Reset,
    State(u8),
}

fn parse_acpi_state(raw: &str) -> Option<AcpiAction> {
    let s = raw.trim();
    if s.eq_ignore_ascii_case("reboot") {
        return Some(AcpiAction::Reset);
    }
    if let Some(rest) = s.strip_prefix('s').or_else(|| s.strip_prefix('S')) {
        return match rest {
            "0" => Some(AcpiAction::State(0)),
            "1" => Some(AcpiAction::State(1)),
            "2" => Some(AcpiAction::State(2)),
            "3" => Some(AcpiAction::State(3)),
            "4" => Some(AcpiAction::State(4)),
            "5" => Some(AcpiAction::State(5)),
            _ => None,
        };
    }
    None
}

fn print_usage(io: &'static dyn ShellBackend2) {
    print_shell_line(io, "acpi: usage `acpi reboot|S1|S2|S3|S4|S5`");
    print_shell_line(io, "acpi: reboot=S0 reset, S5=shutdown");
}

pub(crate) fn try_parse(
    io: &'static dyn ShellBackend2,
    args: &mut SplitWhitespace<'_>,
) -> ParseOutcome {
    let Some(state) = args.next() else {
        print_usage(io);
        return ParseOutcome::Handled;
    };
    if args.next().is_some() {
        print_usage(io);
        return ParseOutcome::Handled;
    }

    let Some(action) = parse_acpi_state(state) else {
        print_usage(io);
        return ParseOutcome::Handled;
    };

    match action {
        AcpiAction::Reset => match crate::efi::acpi::facp::reset_system() {
            Ok(()) => {}
            Err(err) => {
                let msg = alloc::format!("acpi: reset failed ({:?})", err);
                print_shell_line(io, msg.as_str());
            }
        },
        AcpiAction::State(level) => {
            if level == 0 {
                print_shell_line(io, "acpi: already in S0 (running)");
                return ParseOutcome::Handled;
            }
            match crate::efi::acpi::facp::enter_named_sleep_state(level) {
                Ok(()) => {}
                Err(err) => {
                    let msg = alloc::format!("acpi: S{} failed ({:?})", level, err);
                    print_shell_line(io, msg.as_str());
                }
            }
        }
    }

    ParseOutcome::Handled
}
