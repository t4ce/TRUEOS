use core::str::SplitWhitespace;

use super::super::{ShellBackend2, line_width_for_backend, print_shell_line};
use super::tlb_helper::TlbTable;
use crate::shell2::shell2_cmd::ParseOutcome;

fn print_usage(io: &'static dyn ShellBackend2) {
    print_shell_line(io, "smp: usage `smp [slot]`");
}

fn slot_placement(slot: usize) -> alloc::string::String {
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
            .unwrap_or("unclassified-lane");
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

fn concise_trueos_executor_task_name(name: &'static str) -> &'static str {
    let clean = name.strip_suffix('\0').unwrap_or(name);
    clean
        .rsplit("::")
        .find(|part| !part.is_empty() && !part.starts_with('{'))
        .unwrap_or(clean)
}

fn trueos_executor_task(slot: usize) -> alloc::string::String {
    let Some(spawner) = crate::workers::spawner_for_slot(slot as u32) else {
        return alloc::string::String::from("-");
    };
    let spawned = spawner.spawned_task_count();
    let ready = spawner.ready_task_count();
    if let Some(name) = spawner.current_task_name() {
        return alloc::format!(
            "current:{} s={} r={}",
            concise_trueos_executor_task_name(name),
            spawned,
            ready,
        );
    }
    if let Some(name) = spawner.last_task_name() {
        return alloc::format!(
            "last:{} s={} r={}",
            concise_trueos_executor_task_name(name),
            spawned,
            ready,
        );
    }
    alloc::format!("none s={} r={}", spawned, ready)
}

fn service_lane_job(slot: usize) -> alloc::string::String {
    crate::r::blocking::service_lane_activity_text(slot)
        .unwrap_or_else(|| alloc::string::String::from("-"))
}

fn slot_row(slot: usize) -> [alloc::string::String; 6] {
    let Some(r) = crate::smp::read(slot) else {
        return [
            alloc::format!("smp[{}]", slot),
            alloc::string::String::from("off"),
            alloc::string::String::from("-"),
            alloc::string::String::from("-"),
            alloc::string::String::from("-"),
            alloc::string::String::from("-"),
        ];
    };

    [
        alloc::format!("smp[{}]", slot),
        alloc::string::String::from(if r.online { "on" } else { "off" }),
        slot_placement(slot),
        trueos_executor_task(slot),
        service_lane_job(slot),
        crate::smp::hlt_history_text(slot).unwrap_or_else(|| alloc::string::String::from("-")),
    ]
}

fn dump_slots(io: &'static dyn ShellBackend2, slots: core::ops::Range<usize>) {
    const HEADERS: [&str; 6] = [
        "cpu",
        "on",
        "placement",
        "TRUEOS executor task",
        "service-lane job",
        "trace",
    ];
    let table = TlbTable::with_width(&HEADERS, line_width_for_backend(io).saturating_sub(2))
        .with_max_col_widths(&[7, 3, 24, 38, 56, 0]);
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
        "smp: cpu_count={} hlt_hist={}x{}ms samples={} (.=HLT !=sampled-non-HLT; placement/current are live, trace is history)",
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
