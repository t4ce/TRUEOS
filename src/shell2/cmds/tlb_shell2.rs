use core::str::SplitWhitespace;

use super::super::{ShellBackend2, print_shell_line};
use super::tlb_helper::TlbTable;
use crate::shell2::shell2_cmd::ParseOutcome;

const TLB_HEADERS: [&str; 2] = ["Subcommand", "Description"];
const TLB_ROWS: [(&str, &str); 10] = [
    ("pci", "List PCI devices"),
    ("pciids", "Download pci.ids once"),
    ("pci.bar", "List PCI BAR windows"),
    ("mem", "List memory map"),
    ("cpu", "List CPU cores"),
    ("acpi", "List ACPI tables"),
    ("uefi", "List UEFI tables"),
    ("x2apic", "List x2APIC topology"),
    ("usb", "List USB controllers and ports"),
    ("dump", "Write all tables to trueos/pci/tlb.txt"),
];

fn print_menu(io: &'static dyn ShellBackend2) {
    let preview_rows = TLB_ROWS.map(|(cmd, desc)| [cmd, desc]);
    let preview_refs = preview_rows.each_ref().map(|row| row.as_slice());
    let table = TlbTable::autosize(&TLB_HEADERS, &preview_refs, 96);
    table.print_header(io);
    for (cmd, desc) in TLB_ROWS {
        table.print_row(io, &[cmd, desc]);
    }
    table.print_footer(io);
}

fn print_usage(io: &'static dyn ShellBackend2) {
    print_shell_line(
        io,
        "tlb: usage `tlb [pci|pciids|pci.bar|mem|cpu|acpi|uefi|x2apic|usb|dump]`",
    );
}

pub(crate) fn try_parse(
    io: &'static dyn ShellBackend2,
    args: &mut SplitWhitespace<'_>,
) -> ParseOutcome {
    let Some(cmd) = args.next() else {
        print_menu(io);
        return ParseOutcome::Handled;
    };

    if args.next().is_some() {
        print_usage(io);
        return ParseOutcome::Handled;
    }

    match cmd {
        "help" => {
            print_menu(io);
        }
        "pci" | "pciids" | "pci.bar" | "mem" | "cpu" | "acpi" | "uefi" | "x2apic" | "usb"
        | "dump" => {
            let msg = alloc::format!("tlb.{}: not wired in shell2 yet", cmd);
            print_shell_line(io, msg.as_str());
        }
        _ => print_usage(io),
    }

    ParseOutcome::Handled
}
