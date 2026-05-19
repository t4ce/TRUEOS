use alloc::string::String;
use core::fmt::Write;
use core::str::SplitWhitespace;

use embassy_executor::Spawner;

use super::super::{ShellBackend2, print_shell_line};
use super::tlb_helper::print_table;
use crate::shell2::shell2_cmd::ParseOutcome;

const HV_MENU_HEADERS: [&str; 2] = ["Subcommand", "Description"];
const HV_MENU_ROWS: [[&str; 2]; 7] = [
    ["status", "Show VM slot and shared VM resource status"],
    ["run [id] [args...]", "Launch a blueprint in an app VM"],
    ["attach <id>", "Route this matrix slot's input to vm[id]"],
    [
        "detach <id>",
        "Stop routing this matrix slot's input to vm[id]",
    ],
    ["pause <id>", "Request vm[id] stop"],
    ["stop [id]", "Request vm[id] stop"],
    ["preserve <id>", "Sleep vm[id] into the HV ramdisk TRUEOSFS"],
];

const HV_DEFAULT_VM_ID: u8 = 0;

#[inline]
fn print_usage(io: &'static dyn ShellBackend2) {
    print_table(io, &HV_MENU_HEADERS, &HV_MENU_ROWS);
}

fn line(io: &'static dyn ShellBackend2, text: &str) {
    print_shell_line(io, text);
}

fn vm_state_text(vm_id: u8) -> String {
    let state = crate::hv::vm_state(vm_id);
    if !state.supported {
        return alloc::format!("vm{} unsupported", vm_id);
    }
    let lifecycle = if state.running {
        "running"
    } else if state.starting {
        "starting"
    } else {
        "offline"
    };
    let mut suffix = String::new();
    if state.stop_requested {
        suffix.push_str(" stop-pending");
    }
    if state.preserve_requested || state.preserve_exit {
        suffix.push_str(" preserve-pending");
    }
    alloc::format!("vm{} {}{}", state.id, lifecycle, suffix)
}

fn print_available_vms(io: &'static dyn ShellBackend2) {
    line(io, "hv: available app VMs");
    line(io, alloc::format!("hv:   {}", vm_state_text(HV_DEFAULT_VM_ID)).as_str());
}

fn parse_optional_vm_id(io: &'static dyn ShellBackend2, raw: Option<&str>) -> Option<u8> {
    let Some(token) = raw else {
        print_available_vms(io);
        return Some(HV_DEFAULT_VM_ID);
    };

    let Ok(id) = token.parse::<u8>() else {
        line(io, alloc::format!("hv: invalid vm id '{}'", token).as_str());
        print_available_vms(io);
        return None;
    };

    if !crate::hv::vm_state(id).supported {
        line(io, alloc::format!("hv: vm{} is not available yet", id).as_str());
        print_available_vms(io);
        return None;
    }

    Some(id)
}

fn format_bytes(bytes: usize) -> String {
    const KIB: usize = 1024;
    const MIB: usize = 1024 * KIB;
    const GIB: usize = 1024 * MIB;

    if bytes >= GIB {
        alloc::format!("{} GiB", bytes / GIB)
    } else if bytes >= MIB {
        alloc::format!("{} MiB", bytes / MIB)
    } else if bytes >= KIB {
        alloc::format!("{} KiB", bytes / KIB)
    } else {
        alloc::format!("{} B", bytes)
    }
}

fn active_vm_ids_text(status: &crate::hv::HvStatus) -> String {
    let mut out = String::new();
    for maybe_id in status.active_vm_ids {
        if let Some(id) = maybe_id {
            if !out.is_empty() {
                out.push(',');
            }
            let _ = write!(out, "{}", id);
        }
    }
    if out.is_empty() {
        out.push('-');
    }
    out
}

fn print_status(io: &'static dyn ShellBackend2) {
    let status = crate::hv::status();
    let heap_used = status
        .vm_shared_heap_total_bytes
        .saturating_sub(status.vm_shared_heap_free_bytes);

    line(
        io,
        alloc::format!(
            "hv: vm slots running={} starting={} limit={} active={}",
            status.running_count,
            status.starting_count,
            status.vm_id_limit,
            active_vm_ids_text(&status)
        )
        .as_str(),
    );
    line(
        io,
        alloc::format!(
            "hv: vm shared heap used={} total={} free={}",
            format_bytes(heap_used),
            format_bytes(status.vm_shared_heap_total_bytes),
            format_bytes(status.vm_shared_heap_free_bytes)
        )
        .as_str(),
    );
    line(
        io,
        alloc::format!(
            "hv: vm shared stack={} vmx_state={} stored_snapshots={}",
            format_bytes(status.vm_shared_stack_bytes),
            format_bytes(status.vm_shared_vmx_bytes),
            status.stored_vm_count
        )
        .as_str(),
    );
    line(
        io,
        alloc::format!(
            "hv: vmx vendor_intel={} has_vmx={} feature_control_locked={} outside_smx={} guest_module={}",
            status.vendor_intel,
            status.has_vmx,
            status.feature_control_locked,
            status.feature_control_vmx_outside_smx,
            status.guest_module_present
        )
        .as_str(),
    );
}

pub(crate) fn try_parse(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    args: &mut SplitWhitespace<'_>,
) -> ParseOutcome {
    let op = args.next().unwrap_or("status").trim();

    if op.is_empty() || op.eq_ignore_ascii_case("status") {
        if args.next().is_some() {
            print_usage(io);
            return ParseOutcome::Handled;
        }
        print_status(io);
        return ParseOutcome::Handled;
    }

    if op.eq_ignore_ascii_case("run") {
        super::run::try_parse(spawner, io, args);
        return ParseOutcome::Handled;
    }

    if op.eq_ignore_ascii_case("attach") {
        let vm_id = match parse_optional_vm_id(io, args.next()) {
            Some(id) => id,
            None => return ParseOutcome::Handled,
        };
        if args.next().is_some() {
            print_usage(io);
            return ParseOutcome::Handled;
        }
        let state = crate::hv::vm_state(vm_id);
        if !state.running && !state.starting {
            line(io, alloc::format!("hv: vm{} is not running", vm_id).as_str());
            return ParseOutcome::Handled;
        }
        let target = crate::shell2::matrix_target_for_backend(io);
        crate::shell2::bind_matrix_target_vm(&target, vm_id);
        line(io, alloc::format!("hv: bound this matrix slot to vm{}", vm_id).as_str());
        return ParseOutcome::Handled;
    }

    if op.eq_ignore_ascii_case("detach") {
        let vm_id = match parse_optional_vm_id(io, args.next()) {
            Some(id) => id,
            None => return ParseOutcome::Handled,
        };
        if args.next().is_some() {
            print_usage(io);
            return ParseOutcome::Handled;
        }
        let target = crate::shell2::matrix_target_for_backend(io);
        crate::shell2::unbind_matrix_target_vm(&target, vm_id);
        line(io, alloc::format!("hv: detached this matrix slot from vm{} input", vm_id).as_str());
        return ParseOutcome::Handled;
    }

    if op.eq_ignore_ascii_case("pause") {
        let vm_id = match parse_optional_vm_id(io, args.next()) {
            Some(id) => id,
            None => return ParseOutcome::Handled,
        };
        if args.next().is_some() {
            print_usage(io);
            return ParseOutcome::Handled;
        }
        match crate::hv::stop(vm_id) {
            Ok(true) => line(io, alloc::format!("hv: vm{} pause requested", vm_id).as_str()),
            Ok(false) => line(io, alloc::format!("hv: vm{} not running", vm_id).as_str()),
            Err(e) => line(io, alloc::format!("hv: pause failed: {:?}", e).as_str()),
        }
        return ParseOutcome::Handled;
    }

    if op.eq_ignore_ascii_case("stop") {
        let vm_id = match parse_optional_vm_id(io, args.next()) {
            Some(id) => id,
            None => return ParseOutcome::Handled,
        };
        if args.next().is_some() {
            print_usage(io);
            return ParseOutcome::Handled;
        }
        match crate::hv::stop(vm_id) {
            Ok(true) => line(io, alloc::format!("hv: vm{} stop requested", vm_id).as_str()),
            Ok(false) => line(io, alloc::format!("hv: vm{} not running", vm_id).as_str()),
            Err(e) => line(io, alloc::format!("hv: stop failed: {:?}", e).as_str()),
        }
        return ParseOutcome::Handled;
    }

    if op.eq_ignore_ascii_case("preserve") {
        let vm_id = match parse_optional_vm_id(io, args.next()) {
            Some(id) => id,
            None => return ParseOutcome::Handled,
        };
        if args.next().is_some() {
            print_usage(io);
            return ParseOutcome::Handled;
        }
        match crate::hv::request_preserve(vm_id) {
            Ok(true) => line(io, alloc::format!("hv: vm{} preserve requested", vm_id).as_str()),
            Ok(false) => match crate::hv::save_snapshot(vm_id) {
                Ok(bytes) => line(
                    io,
                    alloc::format!(
                        "hv: vm{} snapshot saved store=hv-ramdisk path=vm/vm{}.snapshot bytes={}",
                        vm_id,
                        vm_id,
                        bytes
                    )
                    .as_str(),
                ),
                Err(e) => line(io, alloc::format!("hv: preserve failed: {:?}", e).as_str()),
            },
            Err(e) => line(io, alloc::format!("hv: preserve failed: {:?}", e).as_str()),
        }
        return ParseOutcome::Handled;
    }

    print_usage(io);
    ParseOutcome::Handled
}
