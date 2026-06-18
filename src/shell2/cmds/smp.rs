use core::str::SplitWhitespace;

use super::super::{ShellBackend2, line_width_for_backend, print_shell_line};
use super::tlb_helper::TlbTable;
use crate::shell2::shell2_cmd::ParseOutcome;

fn print_usage(io: &'static dyn ShellBackend2) {
    print_shell_line(io, "smp: usage `smp [slot]`");
}

fn slot_owner(slot: usize) -> alloc::string::String {
    if let Some(vm_id) = crate::hv::vm_id_for_cpu_slot(slot) {
        let archive =
            crate::hv::app_vm_archive(vm_id).unwrap_or_else(|| alloc::string::String::from("-"));
        return alloc::format!("vm{}:{}", vm_id, archive);
    }

    if let Some(vm_id) = crate::hv::lane::vm_owner_for_slot(slot) {
        let archive =
            crate::hv::app_vm_archive(vm_id).unwrap_or_else(|| alloc::string::String::from("-"));
        let label = crate::hv::lane::role_for_slot(slot)
            .map(|role| role.owner_label())
            .unwrap_or("worker");
        if archive != "-" {
            if label == "hull" {
                return alloc::format!("vm{}:{}", vm_id, archive);
            }
            return alloc::format!("vm{}:{}.{}", vm_id, archive, label);
        }
        return alloc::format!("vm{}:{}", vm_id, label);
    }

    if crate::r::blocking::service_lane_started_for_slot(slot) {
        return alloc::string::String::from("service-lane");
    }

    alloc::string::String::from("-")
}

fn slot_row(slot: usize) -> [alloc::string::String; 4] {
    let Some(r) = crate::smp::read(slot) else {
        return [
            alloc::format!("smp[{}]", slot),
            alloc::string::String::from("off"),
            alloc::string::String::from("-"),
            alloc::string::String::from("-"),
        ];
    };

    [
        alloc::format!("smp[{}]", slot),
        alloc::string::String::from(if r.online { "on" } else { "off" }),
        slot_owner(slot),
        crate::smp::hlt_history_text(slot).unwrap_or_else(|| alloc::string::String::from("-")),
    ]
}

fn dump_slots(io: &'static dyn ShellBackend2, slots: core::ops::Range<usize>) {
    const HEADERS: [&str; 4] = ["cpu", "on", "owner", "trace"];
    let table = TlbTable::with_width(&HEADERS, line_width_for_backend(io).saturating_sub(2))
        .with_max_col_widths(&[7, 3, 24, 0]);
    table.emit_header(|text| print_shell_line(io, text));
    for slot in slots {
        let row = slot_row(slot);
        table.emit_row(&row, |text| print_shell_line(io, text));
    }
    table.emit_footer(|text| print_shell_line(io, text));
}

pub(crate) fn try_parse(
    io: &'static dyn ShellBackend2,
    args: &mut SplitWhitespace<'_>,
) -> ParseOutcome {
    if !crate::smp::is_init() {
        print_shell_line(io, "smp: not initialized");
        return ParseOutcome::Handled;
    }

    let total = crate::smp::cpu_count();
    let count_msg = alloc::format!(
        "smp: cpu_count={} hlt_hist={}x{}ms samples={} (.=hlt !=hot)",
        total,
        crate::smp::HLT_HISTORY_LEN,
        crate::smp::HLT_SAMPLE_MS,
        crate::smp::hlt_sample_count()
    );
    print_shell_line(io, count_msg.as_str());

    if let Some(raw_slot) = args.next() {
        let Ok(slot) = raw_slot.parse::<usize>() else {
            print_usage(io);
            return ParseOutcome::Handled;
        };
        if args.next().is_some() {
            print_usage(io);
            return ParseOutcome::Handled;
        }
        if slot >= total {
            print_usage(io);
            return ParseOutcome::Handled;
        }

        dump_slots(io, slot..slot + 1);
        return ParseOutcome::Handled;
    }

    dump_slots(io, 0..total);

    ParseOutcome::Handled
}
