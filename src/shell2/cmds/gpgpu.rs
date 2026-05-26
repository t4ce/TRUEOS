use alloc::format;

use super::super::{ShellBackend2, print_shell_line};
use crate::shell2::shell2_cmd::ParseOutcome;

pub(crate) fn try_parse(io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    match args.next() {
        Some("status") => status(io),
        Some("mandel16") => mandel16(io, &mut args),
        _ => usage(io),
    }
    ParseOutcome::Handled
}

fn usage(io: &'static dyn ShellBackend2) {
    print_shell_line(
        io,
        "gpgpu: usage `gpgpu status` | `gpgpu mandel16 [current|add|mul|mov9|mulimm|mulud|mulacc|mulnop|mulscalar|mulw|muluw|mul8x2|mulwwide|muluwwide] [row] [x]`",
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

fn mandel16(io: &'static dyn ShellBackend2, args: &mut core::str::SplitWhitespace<'_>) {
    let first = args.next();
    let (mode, variant, row_arg) = match first {
        Some("current") => (0, "current", args.next()),
        Some("add") => (1, "add", args.next()),
        Some("mul") => (2, "mul", args.next()),
        Some("mov9") => (3, "mov9", args.next()),
        Some("mulimm") => (4, "mulimm", args.next()),
        Some("mulud") => (5, "mulud", args.next()),
        Some("mulacc") => (6, "mulacc", args.next()),
        Some("mulnop") => (7, "mulnop", args.next()),
        Some("mulscalar") => (8, "mulscalar", args.next()),
        Some("mulw") => (9, "mulw", args.next()),
        Some("muluw") => (10, "muluw", args.next()),
        Some("mul8x2") => (11, "mul8x2", args.next()),
        Some("mulwwide") => (12, "mulwwide", args.next()),
        Some("muluwwide") => (13, "muluwwide", args.next()),
        Some(value) => (0, "current", Some(value)),
        None => (0, "current", None),
    };
    let row = parse_u32(row_arg).unwrap_or(704);
    let x = parse_u32(args.next()).unwrap_or(1272);
    let proof =
        crate::intel::submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_probe(mode, row, x);
    print_shell_line(
        io,
        format!(
            "gpgpu: mandel16 variant={} submitted={} finished={} readback_ok={} reason={} program={} output_gpu=0x{:X} first_before=0x{:08X} first_after=0x{:08X} expected=0x{:08X} hits=0x{:016X} dispatch_delta={} finish=0x{:08X}/0x{:08X} batch_bytes=0x{:X}",
            variant,
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
