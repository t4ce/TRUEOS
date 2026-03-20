use core::str::SplitWhitespace;

use embassy_executor::Spawner;

use super::super::ShellBackend2;
use super::tlb_helper::print_table;
use crate::shell2::shell2_cmd::ParseOutcome;

const HV_MENU_HEADERS: [&str; 2] = ["Subcommand", "Description"];
const HV_MENU_ROWS: [[&str; 2]; 6] = [
    ["status", "Show VMX and vm1 status"],
    ["start", "Start vm1"],
    ["stop", "Request vm1 stop"],
    ["log", "Print hv log output"],
    [
        "save",
        "Write vm1 snapshot to HV ramdisk TRUEOSFS (preserve path: exit and guest halt then save)",
    ],
    [
        "restore",
        "Load vm1 snapshot from the HV ramdisk and relaunch it",
    ],
];

#[inline]
fn print_usage(io: &'static dyn ShellBackend2) {
    print_table(io, &HV_MENU_HEADERS, &HV_MENU_ROWS);
}

pub(crate) fn try_parse(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    args: &mut SplitWhitespace<'_>,
) -> ParseOutcome {
    let op = args.next().unwrap_or("status").trim();

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
            "hv: vm1 running={} starting={} marker_seen={} guest_module={}\r\n",
            s.vm1_running as u8,
            s.vm1_starting as u8,
            s.vm1_marker_seen as u8,
            s.guest_module_present as u8
        ));
        return ParseOutcome::Handled;
    }

    if op.eq_ignore_ascii_case("start") {
        match crate::hv::start(spawner, io) {
            Ok(()) => io.write_str("hv: vm1 started\r\n"),
            Err(e) => io.write_fmt(format_args!("hv: start failed: {:?}\r\n", e)),
        }
        return ParseOutcome::Handled;
    }

    if op.eq_ignore_ascii_case("stop") {
        if crate::hv::stop() {
            io.write_str("hv: vm1 stop requested\r\n");
        } else {
            io.write_str("hv: vm1 not running\r\n");
        }
        return ParseOutcome::Handled;
    }

    if op.eq_ignore_ascii_case("log") {
        crate::hv::write_logs(io);
        return ParseOutcome::Handled;
    }

    if op.eq_ignore_ascii_case("save") {
        match crate::hv::save_snapshot() {
            Ok(bytes) => io.write_fmt(format_args!(
                "hv: snapshot saved store=hv-ramdisk path=vm/vm1.snapshot bytes={}\r\n",
                bytes
            )),
            Err(e) => io.write_fmt(format_args!("hv: snapshot save failed: {:?}\r\n", e)),
        }
        return ParseOutcome::Handled;
    }

    if op.eq_ignore_ascii_case("restore") {
        match crate::hv::restore_snapshot() {
            Ok(bytes) => match crate::hv::start(spawner, io) {
                Ok(()) => io.write_fmt(format_args!(
                    "hv: snapshot restored store=hv-ramdisk path=vm/vm1.snapshot bytes={} and vm1 started\r\n",
                    bytes
                )),
                Err(e) => io.write_fmt(format_args!(
                    "hv: snapshot restored bytes={} but start failed: {:?}\r\n",
                    bytes, e
                )),
            },
            Err(e) => io.write_fmt(format_args!("hv: snapshot restore failed: {:?}\r\n", e)),
        }
        return ParseOutcome::Handled;
    }

    print_usage(io);
    ParseOutcome::Handled
}
