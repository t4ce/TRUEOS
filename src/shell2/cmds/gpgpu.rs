use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};

use super::super::{ShellBackend2, print_shell_line};
use crate::shell2::shell2_cmd::ParseOutcome;

static GPGPU_WALKROW_MAX_TICKS: AtomicU64 = AtomicU64::new(0);
static GPGPU_TILEWALKER_MAX_TICKS: AtomicU64 = AtomicU64::new(0);

pub(crate) fn try_parse(io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    match args.next() {
        Some("status") => status(io),
        Some("walkrow") => walkrow(io, &mut args),
        Some("tilewalker") => tilewalker(io, &mut args),
        _ => usage(io),
    }
    ParseOutcome::Handled
}

fn usage(io: &'static dyn ShellBackend2) {
    print_shell_line(
        io,
        "gpgpu: usage `gpgpu status` | `gpgpu walkrow [row=1..1440] [x=0] [color=0xFFFFFFFF] [stamps=33] [verify=0]` | `gpgpu tilewalker [row=1..1440] [rows=16] [x=0] [color=0xFFFFFFFF] [stamps=33] [verify=0]`",
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
    let stamps = parse_u32(args.next()).unwrap_or(33).clamp(1, 256);
    let verify_readback = parse_bool(args.next()).unwrap_or(false);
    if !(1..=1440).contains(&row) {
        print_shell_line(io, "gpgpu: walkrow row must be in 1..=1440");
        return;
    }

    let start_tick = embassy_time_driver::now();
    let mut stamp = 0u32;
    let mut ok = 0u32;
    let mut last_proof = None;
    while stamp < stamps {
        let stamp_x = x.saturating_add(stamp.saturating_mul(16));
        let proof = crate::intel::submit_gpgpu_primary_scanout_walkrow16(
            row,
            stamp_x,
            color,
            verify_readback,
        );
        if proof.readback_ok {
            ok = ok.saturating_add(1);
        }
        last_proof = Some((stamp_x, proof));
        stamp += 1;
    }
    let elapsed_ticks = embassy_time_driver::now().saturating_sub(start_tick);
    let max_ticks = update_max_ticks(&GPGPU_WALKROW_MAX_TICKS, elapsed_ticks);
    let elapsed_ms = ticks_to_ms(elapsed_ticks);
    let max_ms = ticks_to_ms(max_ticks);

    let Some((last_x, proof)) = last_proof else {
        print_shell_line(io, "gpgpu: walkrow no stamps submitted");
        return;
    };
    print_shell_line(
        io,
        format!(
            "gpgpu: walkrow simd16-store row={} x={} stamps={} pixels={} color=0x{:08X} verify={} ok={}/{} elapsed_ms={} max_ms={} elapsed_ticks={} max_ticks={} tick_hz={} last_x={} submitted={} finished={} readback_ok={} reason={} program={} output_gpu=0x{:X} first_before=0x{:08X} first_after=0x{:08X} expected=0x{:08X} hits=0x{:016X} dispatch_delta={} finish=0x{:08X}/0x{:08X} batch_bytes=0x{:X}",
            row,
            x,
            stamps,
            stamps.saturating_mul(16),
            color,
            verify_readback as u8,
            ok,
            stamps,
            elapsed_ms,
            max_ms,
            elapsed_ticks,
            max_ticks,
            embassy_time_driver::TICK_HZ,
            last_x,
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

fn tilewalker(io: &'static dyn ShellBackend2, args: &mut core::str::SplitWhitespace<'_>) {
    let start_row = parse_u32(args.next()).unwrap_or(1);
    let requested_rows = parse_u32(args.next()).unwrap_or(16).clamp(1, 1440);
    let x = parse_u32(args.next()).unwrap_or(0);
    let color = parse_u32(args.next()).unwrap_or(0xFFFF_FFFF);
    let stamps = parse_u32(args.next()).unwrap_or(33).clamp(1, 256);
    let verify_readback = parse_bool(args.next()).unwrap_or(false);
    if !(1..=1440).contains(&start_row) {
        print_shell_line(io, "gpgpu: tilewalker row must be in 1..=1440");
        return;
    }

    let rows = core::cmp::min(requested_rows, 1440u32.saturating_sub(start_row).saturating_add(1));
    let start_tick = embassy_time_driver::now();
    let mut row_index = 0u32;
    let mut submits = 0u32;
    let mut ok = 0u32;
    let mut last_row = start_row;
    let mut last_x = x;
    let mut last_proof = None;
    while row_index < rows {
        let row = start_row.saturating_add(row_index);
        let mut stamp = 0u32;
        while stamp < stamps {
            let stamp_x = x.saturating_add(stamp.saturating_mul(16));
            let proof = crate::intel::submit_gpgpu_primary_scanout_walkrow16(
                row,
                stamp_x,
                color,
                verify_readback,
            );
            submits = submits.saturating_add(1);
            if proof.readback_ok {
                ok = ok.saturating_add(1);
            }
            last_row = row;
            last_x = stamp_x;
            last_proof = Some(proof);
            stamp += 1;
        }
        row_index += 1;
    }
    let elapsed_ticks = embassy_time_driver::now().saturating_sub(start_tick);
    let max_ticks = update_max_ticks(&GPGPU_TILEWALKER_MAX_TICKS, elapsed_ticks);
    let elapsed_ms = ticks_to_ms(elapsed_ticks);
    let max_ms = ticks_to_ms(max_ticks);

    let Some(proof) = last_proof else {
        print_shell_line(io, "gpgpu: tilewalker no stamps submitted");
        return;
    };
    print_shell_line(
        io,
        format!(
            "gpgpu: tilewalker stable-simd16 start_row={} rows={} x={} stamps_per_row={} pixels={} color=0x{:08X} verify={} ok={}/{} elapsed_ms={} max_ms={} elapsed_ticks={} max_ticks={} tick_hz={} last_row={} last_x={} submitted={} finished={} readback_ok={} reason={} program={} output_gpu=0x{:X} first_before=0x{:08X} first_after=0x{:08X} expected=0x{:08X} hits=0x{:016X} dispatch_delta={} finish=0x{:08X}/0x{:08X} batch_bytes=0x{:X}",
            start_row,
            rows,
            x,
            stamps,
            rows.saturating_mul(stamps).saturating_mul(16),
            color,
            verify_readback as u8,
            ok,
            submits,
            elapsed_ms,
            max_ms,
            elapsed_ticks,
            max_ticks,
            embassy_time_driver::TICK_HZ,
            last_row,
            last_x,
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

fn parse_bool(value: Option<&str>) -> Option<bool> {
    match value? {
        "1" | "true" | "yes" | "on" | "verify" | "readback" => Some(true),
        "0" | "false" | "no" | "off" | "fast" => Some(false),
        _ => None,
    }
}

fn ticks_to_ms(ticks: u64) -> u64 {
    let hz = embassy_time_driver::TICK_HZ;
    if hz == 0 {
        0
    } else {
        ((ticks as u128).saturating_mul(1000) / hz as u128) as u64
    }
}

fn update_max_ticks(max: &AtomicU64, ticks: u64) -> u64 {
    let mut observed = max.load(Ordering::Relaxed);
    while ticks > observed {
        match max.compare_exchange_weak(observed, ticks, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return ticks,
            Err(next) => observed = next,
        }
    }
    observed
}
