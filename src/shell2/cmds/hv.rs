use core::str::SplitWhitespace;

use embassy_executor::Spawner;

use super::super::ShellBackend2;
use super::tlb_helper::print_table;
use crate::shell2::shell2_cmd::ParseOutcome;

const HV_MENU_HEADERS: [&str; 2] = ["Subcommand", "Description"];
const HV_MENU_ROWS: [[&str; 2]; 6] = [
    ["status", "Show VMX and vm1 status"],
    ["start [id]", "Start vm[id] (default id=0, id 0..10)"],
    ["stop [id]", "Request vm[id] stop (default id=0, id 0..10)"],
    ["log", "Print hv log output"],
    [
        "save [id]",
        "Write vm[id] snapshot to HV ramdisk TRUEOSFS (default id=0, id 0..10; preserve path: exit and guest halt then save)",
    ],
    [
        "restore [id]",
        "Load vm[id] snapshot from HV ramdisk and relaunch it (default id=0, id 0..10)",
    ],
];

const HV_DEFAULT_VM_ID: u8 = 0;
const HV_MAX_VM_ID: u8 = 10;

#[inline]
fn print_usage(io: &'static dyn ShellBackend2) {
    print_table(io, &HV_MENU_HEADERS, &HV_MENU_ROWS);
}

fn parse_optional_vm_id(io: &'static dyn ShellBackend2, raw: Option<&str>) -> Option<u8> {
    let Some(token) = raw else {
        return Some(HV_DEFAULT_VM_ID);
    };

    let Ok(id) = token.parse::<u8>() else {
        io.write_fmt(format_args!(
            "hv: invalid vm id '{}' (expected 0..={})\r\n",
            token, HV_MAX_VM_ID
        ));
        return None;
    };

    if id > HV_MAX_VM_ID {
        io.write_fmt(format_args!(
            "hv: vm id {} out of range (expected 0..={})\r\n",
            id, HV_MAX_VM_ID
        ));
        return None;
    }

    Some(id)
}

pub(crate) fn try_parse(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    args: &mut SplitWhitespace<'_>,
) -> ParseOutcome {
    let op = args.next().unwrap_or("status").trim();
    let vm_id = match parse_optional_vm_id(io, args.next()) {
        Some(id) => id,
        None => return ParseOutcome::Handled,
    };

    if args.next().is_some() {
        print_usage(io);
        return ParseOutcome::Handled;
    }

    if op.is_empty() || op.eq_ignore_ascii_case("status") {
        let s = crate::hv::status();
        io.write_fmt(format_args!(
            "hv: vmx intel={} msr={} vmx={} fc_lock={} fc_vmx_outside_smx={}\r\n",
            s.vendor_intel as u8,
            s.has_msr as u8,
            s.has_vmx as u8,
            s.feature_control_locked as u8,
            s.feature_control_vmx_outside_smx as u8
        ));
        io.write_fmt(format_args!(
            "hv: vm1 running={} starting={} marker_seen={} guest_module={} stored_vms={}\r\n",
            s.vm1_running as u8,
            s.vm1_starting as u8,
            s.vm1_marker_seen as u8,
            s.guest_module_present as u8,
            s.stored_vm_count
        ));
        return ParseOutcome::Handled;
    }

    if op.eq_ignore_ascii_case("start") {
        match crate::hv::start(vm_id, spawner, io) {
            Ok(()) => io.write_fmt(format_args!("hv: vm{} started\r\n", vm_id)),
            Err(e) => io.write_fmt(format_args!("hv: start failed: {:?}\r\n", e)),
        }
        return ParseOutcome::Handled;
    }

    if op.eq_ignore_ascii_case("stop") {
        match crate::hv::stop(vm_id) {
            Ok(true) => io.write_fmt(format_args!("hv: vm{} stop requested\r\n", vm_id)),
            Ok(false) => io.write_fmt(format_args!("hv: vm{} not running\r\n", vm_id)),
            Err(e) => io.write_fmt(format_args!("hv: stop failed: {:?}\r\n", e)),
        }
        return ParseOutcome::Handled;
    }

    if op.eq_ignore_ascii_case("log") {
        crate::hv::write_logs(io);
        return ParseOutcome::Handled;
    }

    if op.eq_ignore_ascii_case("save") {
        match crate::hv::save_snapshot(vm_id) {
            Ok(bytes) => io.write_fmt(format_args!(
                "hv: vm{} snapshot saved store=hv-ramdisk path=vm/vm{}.snapshot bytes={}\r\n",
                vm_id, vm_id, bytes
            )),
            Err(e) => io.write_fmt(format_args!("hv: snapshot save failed: {:?}\r\n", e)),
        }
        return ParseOutcome::Handled;
    }

    if op.eq_ignore_ascii_case("restore") {
        match crate::hv::restore_snapshot(vm_id) {
            Ok(bytes) => match crate::hv::start(vm_id, spawner, io) {
                Ok(()) => io.write_fmt(format_args!(
                    "hv: vm{} snapshot restored store=hv-ramdisk path=vm/vm{}.snapshot bytes={} and vm{} started\r\n",
                    vm_id, vm_id, bytes, vm_id
                )),
                Err(e) => io.write_fmt(format_args!(
                    "hv: vm{} snapshot restored bytes={} but start failed: {:?}\r\n",
                    vm_id, bytes, e
                )),
            },
            Err(e) => io.write_fmt(format_args!("hv: snapshot restore failed: {:?}\r\n", e)),
        }
        return ParseOutcome::Handled;
    }

    print_usage(io);
    ParseOutcome::Handled
}
