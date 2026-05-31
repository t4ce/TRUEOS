use alloc::format;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use super::super::{ShellBackend2, print_shell_line};
use crate::shell2::shell2_cmd::ParseOutcome;

static GPGPU_WALKROW_MAX_TICKS: AtomicU64 = AtomicU64::new(0);
static GPGPU_TILEWALKER_MAX_TICKS: AtomicU64 = AtomicU64::new(0);
static GPGPU_ROWBURST_COMPAT_MAX_TICKS: AtomicU64 = AtomicU64::new(0);
static GPGPU_FILL_COLOR_INDEX: AtomicU32 = AtomicU32::new(0);
const GPGPU_CHUNKSTAMP_16PX_STAMPS: u32 = 44;
const GPGPU_STAMP_PIXELS: u32 = 16;
const GPGPU_CHUNKSTAMP_PIXELS: u32 = GPGPU_CHUNKSTAMP_16PX_STAMPS * GPGPU_STAMP_PIXELS;
const GPGPU_ROWBURST_BAND_PIXELS: u32 = 1280;

pub(crate) fn try_parse(io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    match args.next() {
        Some("status") => status(io),
        Some("walkrow") => walkrow(io, &mut args),
        Some("tilewalker") => tilewalker(io, &mut args),
        Some("rowburst") => rowburst(io, &mut args),
        Some("replay") => replay(io, &mut args),
        Some(row) => match parse_u32(Some(row)) {
            Some(row) => rowpaint(io, row, &mut args),
            None => usage(io),
        },
        _ => usage(io),
    }
    ParseOutcome::Handled
}

fn usage(io: &'static dyn ShellBackend2) {
    print_shell_line(
        io,
        "gpgpu: usage `gpgpu [row=1..1440] [rows=5]` | `gpgpu status` | debug: `gpgpu walkrow ...` `gpgpu tilewalker ...` `gpgpu rowburst ...` `gpgpu replay [0..2]`",
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

fn rowpaint(
    io: &'static dyn ShellBackend2,
    start_row: u32,
    args: &mut core::str::SplitWhitespace<'_>,
) {
    let requested_rows = parse_u32(args.next()).unwrap_or(5).clamp(1, 1440);
    let color = next_fill_color();
    if !(1..=1440).contains(&start_row) {
        print_shell_line(io, "gpgpu: row must be in 1..=1440");
        return;
    }

    let (width, height) = crate::intel::active_scanout_dimensions().unwrap_or((2560, 1440));
    let rows = core::cmp::min(requested_rows, height.saturating_sub(start_row).saturating_add(1));
    let start_tick = embassy_time_driver::now();
    let mut submits = 0u32;
    let mut ok = 0u32;
    let mut last_row = start_row;
    let mut last_proof = None;

    let mut row_index = 0u32;
    while row_index < rows {
        let row = start_row.saturating_add(row_index);
        let proof = crate::intel::submit_gpgpu_primary_scanout_row2560_simd16(row, color);
        submits = submits.saturating_add(1);
        if proof.readback_ok {
            ok = ok.saturating_add(1);
        }
        last_proof = Some(proof);
        last_row = row;
        row_index += 1;
    }

    let elapsed_ticks = embassy_time_driver::now().saturating_sub(start_tick);
    let elapsed_ms = ticks_to_ms(elapsed_ticks);
    let Some(proof) = last_proof else {
        print_shell_line(io, "gpgpu: rowpaint no rows submitted");
        return;
    };
    print_shell_line(
        io,
        format!(
            "gpgpu: rowpaint start={} rows={} width={} auto_rgb=0x{:06X} impl=row2560-simd16 submits={} ok={}/{} elapsed_ms={} elapsed_ticks={} tick_hz={} last_row={} submitted={} finished={} ok_last={} reason={} program={} output_gpu=0x{:X} expected=0x{:08X} dispatch_delta={} finish=0x{:08X}/0x{:08X} batch_bytes=0x{:X}",
            start_row,
            rows,
            width,
            color & 0x00FF_FFFF,
            submits,
            ok,
            submits,
            elapsed_ms,
            elapsed_ticks,
            embassy_time_driver::TICK_HZ,
            last_row,
            proof.submitted as u8,
            proof.finished as u8,
            proof.readback_ok as u8,
            proof.reason,
            proof.program_name,
            proof.output_gpu,
            proof.sentinel,
            proof.dispatch_delta,
            proof.finish_marker,
            proof.expected_finish_marker,
            proof.batch_bytes,
        )
        .as_str(),
    );
}

fn next_fill_color() -> u32 {
    const COLORS: [u32; 8] = [
        0x00FF00, 0xFF00FF, 0x00FFFF, 0xFF0000, 0x0000FF, 0xFFFF00, 0xFFFFFF, 0x202020,
    ];
    let index = GPGPU_FILL_COLOR_INDEX.fetch_add(1, Ordering::Relaxed) as usize;
    COLORS[index % COLORS.len()]
}

fn replay(io: &'static dyn ShellBackend2, args: &mut core::str::SplitWhitespace<'_>) {
    let frame = parse_u32(args.next()).unwrap_or(0).min(2) as usize;
    let mode_arg = args.next().unwrap_or("visible");
    let (mode_label, load_mode) = match mode_arg {
        "full" => ("full", crate::intel::replay::ReplayModuleLoadMode::Full),
        _ => (
            "visible",
            crate::intel::replay::ReplayModuleLoadMode::VisibleTruncated,
        ),
    };
    print_shell_line(
        io,
        format!(
            "gpgpu: replay rotating-triangle frame={} mode={} allocating captured BO set (~154 MiB)",
            frame, mode_label,
        )
        .as_str(),
    );
    let proof = crate::intel::submit_rotating_triangle_replay_frame(frame, load_mode);
    print_shell_line(
        io,
        format!(
            "gpgpu: replay frame={} submitted={} retired={} batch_gpu=0x{:X} pml4=0x{:X} table_pages={} pre=0x{:08X} post=0x{:08X} head=0x{:08X} tail=0x{:08X} acthd=0x{:08X}:0x{:08X} bbaddr=0x{:08X}:0x{:08X} ipehr=0x{:08X} eir=0x{:08X} fault8=0x{:08X} fault12=0x{:08X}",
            frame,
            proof.submitted as u8,
            proof.retired as u8,
            proof.batch_gpu,
            proof.pml4_phys,
            proof.table_pages,
            proof.pre_marker,
            proof.post_marker,
            proof.head,
            proof.tail,
            proof.acthd_hi,
            proof.acthd,
            proof.bbaddr_hi,
            proof.bbaddr_lo,
            proof.ipehr,
            proof.eir,
            proof.fault8,
            proof.fault12,
        )
        .as_str(),
    );
}

fn walkrow(io: &'static dyn ShellBackend2, args: &mut core::str::SplitWhitespace<'_>) {
    let row = parse_u32(args.next()).unwrap_or(1);
    let x = parse_u32(args.next()).unwrap_or(0);
    let Some(color) = parse_scanout_rgb24(io, args.next()) else {
        return;
    };
    let stamps_arg = args.next();
    let verify_readback = parse_bool(args.next()).unwrap_or(false);
    let stamps = parse_stamps(stamps_arg, x, verify_readback)
        .unwrap_or(33)
        .clamp(1, 512);
    if !(1..=1440).contains(&row) {
        print_shell_line(io, "gpgpu: walkrow row must be in 1..=1440");
        return;
    }

    let start_tick = embassy_time_driver::now();
    let mut stamp = 0u32;
    let mut ok = 0u32;
    let mut last_proof = None;
    while stamp < stamps {
        if !verify_readback {
            let stamp_x = x.saturating_add(stamp.saturating_mul(GPGPU_CHUNKSTAMP_PIXELS));
            let proof = crate::intel::submit_gpgpu_primary_scanout_chunkstamp704(
                row, stamp_x, color, false,
            );
            if proof.readback_ok {
                ok = ok.saturating_add(1);
            }
            last_proof = Some((
                stamp_x.saturating_add(GPGPU_CHUNKSTAMP_PIXELS.saturating_sub(GPGPU_STAMP_PIXELS)),
                proof,
            ));
            stamp += 1;
        } else {
            let stamp_x = x.saturating_add(stamp.saturating_mul(GPGPU_STAMP_PIXELS));
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
            "gpgpu: walkrow simd16-store row={} x={} stamps={} pixels={} rgb=0x{:06X} pixel=0x{:08X} verify={} ok={}/{} elapsed_ms={} max_ms={} elapsed_ticks={} max_ticks={} tick_hz={} last_x={} submitted={} finished={} readback_ok={} reason={} program={} output_gpu=0x{:X} first_before=0x{:08X} first_after=0x{:08X} expected=0x{:08X} hits=0x{:016X} dispatch_delta={} finish=0x{:08X}/0x{:08X} batch_bytes=0x{:X}",
            row,
            x,
            stamps,
            stamps.saturating_mul(pixels_per_shell_stamp(verify_readback)),
            color & 0x00FF_FFFF,
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
    let Some(color) = parse_scanout_rgb24(io, args.next()) else {
        return;
    };
    let stamps_arg = args.next();
    let verify_readback = parse_bool(args.next()).unwrap_or(false);
    let stamps = parse_stamps(stamps_arg, x, verify_readback)
        .unwrap_or(33)
        .clamp(1, 512);
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
            if !verify_readback {
                let stamp_x = x.saturating_add(stamp.saturating_mul(GPGPU_CHUNKSTAMP_PIXELS));
                let proof = crate::intel::submit_gpgpu_primary_scanout_chunkstamp704(
                    row, stamp_x, color, false,
                );
                submits = submits.saturating_add(1);
                if proof.readback_ok {
                    ok = ok.saturating_add(1);
                }
                last_row = row;
                last_x = stamp_x
                    .saturating_add(GPGPU_CHUNKSTAMP_PIXELS.saturating_sub(GPGPU_STAMP_PIXELS));
                last_proof = Some(proof);
                stamp += 1;
            } else {
                let stamp_x = x.saturating_add(stamp.saturating_mul(GPGPU_STAMP_PIXELS));
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
            "gpgpu: tilewalker stable-simd16 start_row={} rows={} x={} stamps_per_row={} pixels={} rgb=0x{:06X} pixel=0x{:08X} verify={} ok={}/{} elapsed_ms={} max_ms={} elapsed_ticks={} max_ticks={} tick_hz={} last_row={} last_x={} submitted={} finished={} readback_ok={} reason={} program={} output_gpu=0x{:X} first_before=0x{:08X} first_after=0x{:08X} expected=0x{:08X} hits=0x{:016X} dispatch_delta={} finish=0x{:08X}/0x{:08X} batch_bytes=0x{:X}",
            start_row,
            rows,
            x,
            stamps,
            rows.saturating_mul(stamps)
                .saturating_mul(pixels_per_shell_stamp(verify_readback)),
            color & 0x00FF_FFFF,
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

fn rowburst(io: &'static dyn ShellBackend2, args: &mut core::str::SplitWhitespace<'_>) {
    let start_row = parse_u32(args.next()).unwrap_or(1);
    let requested_rows = parse_u32(args.next()).unwrap_or(16).clamp(1, 1440);
    let x = parse_u32(args.next()).unwrap_or(0);
    let Some(color) = parse_scanout_rgb24(io, args.next()) else {
        return;
    };
    let bands_arg = args.next();
    let bands = parse_rowburst_bands(bands_arg, x).unwrap_or(1).clamp(1, 8);
    let mode = parse_rowburst_mode(args.next());
    if !(1..=1440).contains(&start_row) {
        print_shell_line(io, "gpgpu: rowburst row must be in 1..=1440");
        return;
    }

    let rows = core::cmp::min(requested_rows, 1440u32.saturating_sub(start_row).saturating_add(1));
    if !mode.raw_artifact {
        rowburst_compat(io, start_row, rows, x, color, bands, mode.label, mode.compat_phases);
        return;
    }

    let start_tick = embassy_time_driver::now();
    let mut band = 0u32;
    let mut ok = 0u32;
    let mut last_x = x;
    let mut last_proof = None;
    while band < bands {
        let band_x = x.saturating_add(band.saturating_mul(GPGPU_ROWBURST_BAND_PIXELS));
        let proof = crate::intel::submit_gpgpu_primary_scanout_rowburst1280(
            start_row,
            rows,
            band_x,
            color,
            mode.allow_no_eot,
        );
        if proof.readback_ok {
            ok = ok.saturating_add(1);
        }
        last_x = band_x;
        last_proof = Some(proof);
        band += 1;
    }

    let elapsed_ticks = embassy_time_driver::now().saturating_sub(start_tick);
    let elapsed_ms = ticks_to_ms(elapsed_ticks);
    let Some(proof) = last_proof else {
        print_shell_line(io, "gpgpu: rowburst no bands submitted");
        return;
    };
    print_shell_line(
        io,
        format!(
            "gpgpu: rowburst groupid-line1280 start_row={} rows={} x={} bands={} pixels={} rgb=0x{:06X} pixel=0x{:08X} mode={} ok={}/{} elapsed_ms={} elapsed_ticks={} tick_hz={} last_x={} submitted={} finished={} readback_ok={} reason={} program={} output_gpu=0x{:X} expected=0x{:08X} dispatch_delta={} finish=0x{:08X}/0x{:08X} batch_bytes=0x{:X}",
            start_row,
            rows,
            x,
            bands,
            rows.saturating_mul(bands)
                .saturating_mul(GPGPU_ROWBURST_BAND_PIXELS),
            color & 0x00FF_FFFF,
            color,
            mode.label,
            ok,
            bands,
            elapsed_ms,
            elapsed_ticks,
            embassy_time_driver::TICK_HZ,
            last_x,
            proof.submitted as u8,
            proof.finished as u8,
            proof.readback_ok as u8,
            proof.reason,
            proof.program_name,
            proof.output_gpu,
            proof.sentinel,
            proof.dispatch_delta,
            proof.finish_marker,
            proof.expected_finish_marker,
            proof.batch_bytes,
        )
        .as_str(),
    );
}

fn rowburst_compat(
    io: &'static dyn ShellBackend2,
    start_row: u32,
    rows: u32,
    x: u32,
    color: u32,
    bands: u32,
    mode_label: &'static str,
    phases: u32,
) {
    let start_tick = embassy_time_driver::now();
    let mut row_index = 0u32;
    let mut submits = 0u32;
    let mut ok = 0u32;
    let mut last_row = start_row;
    let mut last_x = x;
    let mut last_proof = None;
    let chunks_per_band = GPGPU_ROWBURST_BAND_PIXELS.saturating_add(GPGPU_CHUNKSTAMP_PIXELS - 1)
        / GPGPU_CHUNKSTAMP_PIXELS;
    let scanout_width = crate::intel::active_scanout_dimensions()
        .map(|(width, _)| width)
        .unwrap_or(2560);
    let max_phase_base_x = scanout_width
        .saturating_sub(GPGPU_CHUNKSTAMP_PIXELS)
        .saturating_sub(phases.saturating_sub(1));

    while row_index < rows {
        let row = start_row.saturating_add(row_index);
        let mut band = 0u32;
        while band < bands {
            let band_x = x.saturating_add(band.saturating_mul(GPGPU_ROWBURST_BAND_PIXELS));
            let mut chunk = 0u32;
            while chunk < chunks_per_band {
                let stamp_x = core::cmp::min(
                    band_x.saturating_add(chunk.saturating_mul(GPGPU_CHUNKSTAMP_PIXELS)),
                    max_phase_base_x,
                );
                let mut phase = 0u32;
                while phase < phases {
                    let phase_x = stamp_x.saturating_add(phase);
                    let proof = crate::intel::submit_gpgpu_primary_scanout_chunkstamp704(
                        row, phase_x, color, false,
                    );
                    submits = submits.saturating_add(1);
                    if proof.readback_ok {
                        ok = ok.saturating_add(1);
                    }
                    last_row = row;
                    last_x = phase_x
                        .saturating_add(GPGPU_CHUNKSTAMP_PIXELS.saturating_sub(GPGPU_STAMP_PIXELS));
                    last_proof = Some(proof);
                    phase += 1;
                }
                chunk += 1;
            }
            band += 1;
        }
        row_index += 1;
    }

    let elapsed_ticks = embassy_time_driver::now().saturating_sub(start_tick);
    let max_ticks = update_max_ticks(&GPGPU_ROWBURST_COMPAT_MAX_TICKS, elapsed_ticks);
    let elapsed_ms = ticks_to_ms(elapsed_ticks);
    let max_ms = ticks_to_ms(max_ticks);
    let Some(proof) = last_proof else {
        print_shell_line(io, "gpgpu: rowburst compat no chunks submitted");
        return;
    };
    print_shell_line(
        io,
        format!(
            "gpgpu: rowburst compat-chunkstamp704 start_row={} rows={} x={} bands={} chunks_per_band={} phases={} pixels_target={} pixels_submitted={} rgb=0x{:06X} pixel=0x{:08X} mode={} ok={}/{} elapsed_ms={} max_ms={} elapsed_ticks={} max_ticks={} tick_hz={} last_row={} last_x={} submitted={} finished={} readback_ok={} reason={} program={} output_gpu=0x{:X} expected=0x{:08X} dispatch_delta={} finish=0x{:08X}/0x{:08X} batch_bytes=0x{:X}",
            start_row,
            rows,
            x,
            bands,
            chunks_per_band,
            phases,
            rows.saturating_mul(bands)
                .saturating_mul(GPGPU_ROWBURST_BAND_PIXELS),
            rows.saturating_mul(bands)
                .saturating_mul(chunks_per_band)
                .saturating_mul(phases)
                .saturating_mul(GPGPU_CHUNKSTAMP_PIXELS),
            color & 0x00FF_FFFF,
            color,
            mode_label,
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
            proof.sentinel,
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

fn parse_scanout_rgb24(io: &'static dyn ShellBackend2, value: Option<&str>) -> Option<u32> {
    let raw = match value {
        None => 0x00FF_FFFF,
        Some(value) => parse_u32(Some(value)).or_else(|| parse_bare_hex_u32(value))?,
    };
    if raw <= 0x00FF_FFFF {
        return Some(raw);
    }

    print_shell_line(
        io,
        format!(
            "gpgpu: color is 24-bit RGB for XRGB scanout; use 0x{:06X} instead of 0x{:08X}",
            raw & 0x00FF_FFFF,
            raw,
        )
        .as_str(),
    );
    None
}

fn parse_bare_hex_u32(value: &str) -> Option<u32> {
    if value.is_empty() || !value.as_bytes().iter().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(value, 16).ok()
}

fn parse_bool(value: Option<&str>) -> Option<bool> {
    match value? {
        "1" | "true" | "yes" | "on" | "verify" | "readback" => Some(true),
        "0" | "false" | "no" | "off" | "fast" => Some(false),
        _ => None,
    }
}

fn parse_stamps(value: Option<&str>, x: u32, verify_readback: bool) -> Option<u32> {
    let Some(value) = value else {
        return None;
    };
    match value {
        "full" | "row" | "all" | "max" => Some(full_row_stamps_from_x(x, verify_readback)),
        _ => parse_u32(Some(value)),
    }
}

fn full_row_stamps_from_x(x: u32, verify_readback: bool) -> u32 {
    let width = crate::intel::active_scanout_dimensions()
        .map(|(width, _)| width)
        .unwrap_or(2560);
    let remaining = width.saturating_sub(x).max(1);
    let pixels_per_stamp = pixels_per_shell_stamp(verify_readback);
    remaining.saturating_add(pixels_per_stamp - 1) / pixels_per_stamp
}

fn pixels_per_shell_stamp(verify_readback: bool) -> u32 {
    if verify_readback {
        GPGPU_STAMP_PIXELS
    } else {
        GPGPU_CHUNKSTAMP_PIXELS
    }
}

fn parse_rowburst_bands(value: Option<&str>, x: u32) -> Option<u32> {
    let Some(value) = value else {
        return None;
    };
    match value {
        "full" | "row" | "all" | "max" => {
            let width = crate::intel::active_scanout_dimensions()
                .map(|(width, _)| width)
                .unwrap_or(2560);
            let remaining = width.saturating_sub(x).max(1);
            Some(
                remaining.saturating_add(GPGPU_ROWBURST_BAND_PIXELS - 1)
                    / GPGPU_ROWBURST_BAND_PIXELS,
            )
        }
        _ => parse_u32(Some(value)),
    }
}

#[derive(Clone, Copy)]
struct RowburstMode {
    label: &'static str,
    allow_no_eot: bool,
    raw_artifact: bool,
    compat_phases: u32,
}

fn parse_rowburst_mode(value: Option<&str>) -> RowburstMode {
    match value {
        Some("raw") | Some("experimental") => RowburstMode {
            label: "raw-strict",
            allow_no_eot: false,
            raw_artifact: true,
            compat_phases: 1,
        },
        Some("raw-loose") | Some("experimental-loose") => RowburstMode {
            label: "raw-loose",
            allow_no_eot: true,
            raw_artifact: true,
            compat_phases: 1,
        },
        Some("repair2") | Some("fat2") | Some("overlap2") => RowburstMode {
            label: "compat-repair2",
            allow_no_eot: false,
            raw_artifact: false,
            compat_phases: 2,
        },
        Some("repair4") | Some("fat4") | Some("overlap4") | Some("repair") => RowburstMode {
            label: "compat-repair4",
            allow_no_eot: false,
            raw_artifact: false,
            compat_phases: 4,
        },
        Some("repair8") | Some("fat8") | Some("overlap8") => RowburstMode {
            label: "compat-repair8",
            allow_no_eot: false,
            raw_artifact: false,
            compat_phases: 8,
        },
        Some("repair16") | Some("fat16") | Some("overlap16") | Some("lane16") => RowburstMode {
            label: "compat-repair16",
            allow_no_eot: false,
            raw_artifact: false,
            compat_phases: GPGPU_STAMP_PIXELS,
        },
        Some("repair32") | Some("fat32") | Some("overlap32") | Some("lane32") => RowburstMode {
            label: "compat-repair32",
            allow_no_eot: false,
            raw_artifact: false,
            compat_phases: GPGPU_STAMP_PIXELS * 2,
        },
        Some("loose") | Some("noeot") | Some("no-eot") | Some("dispatch") => RowburstMode {
            label: "compat-loose",
            allow_no_eot: false,
            raw_artifact: false,
            compat_phases: 1,
        },
        _ => RowburstMode {
            label: "compat-strict",
            allow_no_eot: false,
            raw_artifact: false,
            compat_phases: 1,
        },
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
