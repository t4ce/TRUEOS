use core::str::SplitWhitespace;

use super::super::{ShellBackend2, print_shell_line};
use crate::intel::gpgpu::{Eu32Simd16ProbeReport, Eu32Simd16ProbeStatus};
use crate::shell2::shell2_cmd::ParseOutcome;

fn print_usage(io: &'static dyn ShellBackend2) {
    print_shell_line(io, "gpgpu: usage `gpgpu [probe|status]`");
}

fn status_detail(status: Eu32Simd16ProbeStatus) -> &'static str {
    match status {
        Eu32Simd16ProbeStatus::NoIntelDevice => "Intel display-class PCI device is not claimed",
        Eu32Simd16ProbeStatus::GuCNotReady => "GuC is not ready; render submit is unsafe",
        Eu32Simd16ProbeStatus::RenderWalkerNotLinked => {
            "RCS GPGPU_WALKER plus EU ISA entrypoint is not linked in this build"
        }
    }
}

fn print_report(io: &'static dyn ShellBackend2, report: Eu32Simd16ProbeReport) {
    print_shell_line(
        io,
        alloc::format!(
            "gpgpu: eu32-simd16 seq={} status={} submitted={} retired={} reason={}",
            report.seq,
            report.status.as_str(),
            report.submitted as u8,
            report.retired as u8,
            report.reason
        )
        .as_str(),
    );
    print_shell_line(
        io,
        alloc::format!(
            "gpgpu: device=0x{:04X} rev=0x{:02X} guc_ready={} eus={} simd={} lane_mask=0x{:08X}",
            report.device_id,
            report.revision_id,
            report.guc_ready as u8,
            report.requested_eus,
            report.simd_width,
            report.lane_mask
        )
        .as_str(),
    );
    print_shell_line(
        io,
        alloc::format!(
            "gpgpu: cpu-ref add_checksum=0x{:08X} mul_checksum=0x{:08X} add_lane0=0x{:08X} mul_lane0=0x{:08X}",
            report.add_checksum,
            report.mul_checksum,
            report.cpu_add_bits[0],
            report.cpu_mul_bits[0]
        )
        .as_str(),
    );
    print_shell_line(io, alloc::format!("gpgpu: {}", status_detail(report.status)).as_str());
}

pub(crate) fn try_parse(
    io: &'static dyn ShellBackend2,
    args: &mut SplitWhitespace<'_>,
) -> ParseOutcome {
    match args.next() {
        None | Some("probe") | Some("status") => {
            if args.next().is_some() {
                print_usage(io);
                return ParseOutcome::Handled;
            }
            print_report(io, crate::intel::run_gpgpu_eu32_simd16_probe());
        }
        Some(_) => print_usage(io),
    }
    ParseOutcome::Handled
}
