use core::str::SplitWhitespace;

use embassy_executor::Spawner;

use super::super::ShellBackend2;
use super::tlb_helper::print_table;
use crate::shell2::shell2_cmd::ParseOutcome;

const HV_MENU_HEADERS: [&str; 2] = ["Subcommand", "Description"];
const HV_MENU_ROWS: [[&str; 2]; 5] = [
    [
        "full [id]",
        "Start vm[id] with full TRUEOS guest image mapping",
    ],
    ["start [id]", "Start vm[id] as minimal trueos-vm hull guest"],
    ["pause [id]", "Alias for stop request on vm[id]"],
    ["stop [id]", "Request vm[id] stop"],
    [
        "preserve [id]",
        "Save vm[id] snapshot to HV ramdisk TRUEOSFS",
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
    let op = args.next().unwrap_or("start").trim();
    let vm_id = match parse_optional_vm_id(io, args.next()) {
        Some(id) => id,
        None => return ParseOutcome::Handled,
    };

    if args.next().is_some() {
        print_usage(io);
        return ParseOutcome::Handled;
    }

    if op.is_empty() || op.eq_ignore_ascii_case("start") {
        match crate::hv::start(vm_id, spawner, io) {
            Ok(()) => io.write_fmt(format_args!("hv: vm{} started\r\n", vm_id)),
            Err(e) => io.write_fmt(format_args!("hv: start failed: {:?}\r\n", e)),
        }
        return ParseOutcome::Handled;
    }

    if op.eq_ignore_ascii_case("full") {
        match crate::hv::start_full(vm_id, spawner, io) {
            Ok(()) => io.write_fmt(format_args!("hv: vm{} full guest started\r\n", vm_id)),
            Err(e) => io.write_fmt(format_args!("hv: full start failed: {:?}\r\n", e)),
        }
        return ParseOutcome::Handled;
    }

    if op.eq_ignore_ascii_case("pause") {
        match crate::hv::stop(vm_id) {
            Ok(true) => io.write_fmt(format_args!("hv: vm{} pause requested\r\n", vm_id)),
            Ok(false) => io.write_fmt(format_args!("hv: vm{} not running\r\n", vm_id)),
            Err(e) => io.write_fmt(format_args!("hv: pause failed: {:?}\r\n", e)),
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

    if op.eq_ignore_ascii_case("preserve") {
        match crate::hv::save_snapshot(vm_id) {
            Ok(bytes) => io.write_fmt(format_args!(
                "hv: vm{} snapshot saved store=hv-ramdisk path=vm/vm{}.snapshot bytes={}\r\n",
                vm_id, vm_id, bytes
            )),
            Err(e) => io.write_fmt(format_args!("hv: preserve failed: {:?}\r\n", e)),
        }
        return ParseOutcome::Handled;
    }

    print_usage(io);
    ParseOutcome::Handled
}
