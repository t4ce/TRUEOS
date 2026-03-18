use core::str::SplitWhitespace;

use embassy_executor::Spawner;

use super::super::ShellBackend2;
use crate::shell2::shell2_cmd::ParseOutcome;

#[inline]
fn print_usage(io: &'static dyn ShellBackend2) {
    io.write_str("hv: usage hv [status|start|stop|log]\r\n");
    io.write_str("hv: single-VM milestone target is vm1\r\n");
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

    print_usage(io);
    ParseOutcome::Handled
}
