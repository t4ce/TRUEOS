use alloc::format;

use super::super::{ShellBackend2, print_shell_line};
use crate::shell2::shell2_cmd::ParseOutcome;

pub(crate) fn try_parse(io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    match args.next() {
        Some("status") => status(io),
        Some("walkrow") => walkrow(io, &mut args),
        _ => usage(io),
    }
    ParseOutcome::Handled
}

fn usage(io: &'static dyn ShellBackend2) {
    print_shell_line(
        io,
        "gpgpu: usage `gpgpu status` | `gpgpu walkrow [row=1..1440] [x=0] [color=0xFFFFFFFF]`",
    );
}

fn status(io: &'static dyn ShellBackend2) {
    let s = crate::intel::gpgpu_preflight_status();
    print_shell_line(
        io,
        format!(
            "gpgpu: preflight submitted={} accepted={} completed={} eu_uploaded={} walker_encoded={} walker_submitted={} walker_retired={} dispatch_delta={} c=0x{:08X} expected=0x{:08X} program={}",
            s.submitted as u8,
            s.accepted as u8,
            s.completed as u8,
            s.eu_kernel_uploaded as u8,
            s.eu_walker_encoded as u8,
            s.eu_walker_submitted as u8,
            s.eu_walker_retired as u8,
            s.eu_dispatch_delta,
            s.eu_c_store_value,
            s.eu_expected_store_value,
            s.eu_program_name,
        )
        .as_str(),
    );
}

fn walkrow(io: &'static dyn ShellBackend2, args: &mut core::str::SplitWhitespace<'_>) {
    let row = parse_u32(args.next()).unwrap_or(1);
    let x = parse_u32(args.next()).unwrap_or(0);
    let color = parse_u32(args.next()).unwrap_or(0xFFFF_FFFF);
    if !(1..=1440).contains(&row) {
        print_shell_line(io, "gpgpu: walkrow row must be in 1..=1440");
        return;
    }

    let proof = crate::intel::submit_gpgpu_primary_scanout_walkrow16(row, x, color);
    print_shell_line(
        io,
        format!(
            "gpgpu: walkrow simd16-store row={} x={} pixels=16 color=0x{:08X} submitted={} finished={} readback_ok={} reason={} program={} output_gpu=0x{:X} first_before=0x{:08X} first_after=0x{:08X} expected=0x{:08X} hits=0x{:016X} dispatch_delta={} finish=0x{:08X}/0x{:08X} batch_bytes=0x{:X}",
            row,
            x,
            color,
            proof.submitted as u8,
            proof.finished as u8,
            proof.readback_ok as u8,
            proof.reason,
            proof.program_name,
            proof.output_gpu,
            proof.output_first_before,
            proof.output_first_after,
            proof.sentinel,
            proof.output_hits_lo64,
            proof.dispatch_delta,
            proof.finish_marker,
            proof.expected_finish_marker,
            proof.batch_bytes,
        )
        .as_str(),
    );
}

fn parse_u32(value: Option<&str>) -> Option<u32> {
    let value = value?;
    if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        u32::from_str_radix(hex, 16).ok()
    } else {
        value.parse::<u32>().ok()
    }
}
