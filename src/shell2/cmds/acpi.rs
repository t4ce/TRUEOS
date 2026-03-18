
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

pub(crate) fn cmd_acpi(
    ctx: &mut ShellCommandCtx<'_>,
    args: Option<&ParsedArgs<'_>>,
) -> CommandAction {
    let print_usage = |io: &dyn ShellIo| {
        let cols = [
            TableColumn {
                header: "State",
                width: 8,
            },
            TableColumn {
                header: "Description",
                width: 32,
            },
        ];
        let t = Table::new(&cols);
        t.print_header(io);
        t.print_row(io, ["reboot", "ACPI reset"]);
        t.print_row(io, ["S0", "Running"]);
        t.print_row(io, ["S1", "Light sleep"]);
        t.print_row(io, ["S2", "Deeper sleep (rare)"]);
        t.print_row(io, ["S3", "Suspend to RAM"]);
        t.print_row(io, ["S4", "Hibernate (suspend to disk)"]);
        t.print_row(io, ["S5", "Soft off (shutdown)"]);
    };

    let Some(state) = args.and_then(|a| a.get_str(0)) else {
        print_usage(ctx.io);
        return CommandAction::None;
    };

    let Some(action) = parse_acpi_state(state) else {
        print_usage(ctx.io);
        return CommandAction::None;
    };

    match action {
        AcpiAction::Reset => CommandAction::Pending(crate::shell::PendingAction::AcpiReset),
        AcpiAction::State(level) => {
            if level == 0 {
                ctx.io.write_str("acpi: already in S0 (running)\r\n");
                return CommandAction::None;
            }
            CommandAction::Pending(crate::shell::PendingAction::AcpiState(level))
        }
    }
}
