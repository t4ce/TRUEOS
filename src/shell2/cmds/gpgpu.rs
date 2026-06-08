use alloc::string::String as AllocString;
use core::str::SplitWhitespace;
use core::sync::atomic::{AtomicU32, Ordering};

use super::super::{ShellBackend2, print_shell_line};
use crate::intel::gpgpu::{
    GPGPU_SHELL_SURFACE_HEIGHT, GPGPU_SHELL_SURFACE_PITCH_BYTES, GPGPU_SHELL_SURFACE_WIDTH,
    GpgpuPoint, GpgpuRect, MANDEL64_WORKLIST_DEFAULT_ITERATIONS, MANDEL64_WORKLIST_MAX_ITERATIONS,
    alpha_blend_worklist_probe_ok, alpha_blend_worklist_probe_ran,
    alpha_blend_worklist_rgba8_upload_status, canvas3d_plane_fill_rgba8_upload_status,
    canvas3d_project_rgba8_upload_status, canvas3d_transform_q16_upload_status,
    copy_rect_rgba8_upload_status, fill_rect_worklist_probe_ok, fill_rect_worklist_probe_ran,
    fill_rect_worklist_rgba8_upload_status, glyph_mask_rgba8_upload_status,
    gradient_rect_worklist_probe_ok, gradient_rect_worklist_probe_ran,
    gradient_rect_worklist_rgba8_upload_status, mandel64_worklist_rgba8_upload_status,
    present_rgba8_to_primary_xrgb_rect_upload_status, rect_worklist_probe_ready, shell_copy_rgba8,
    shell_copy_scanout_center_rgba8, shell_cube20_project_spin, shell_mandel64_worklist_scanout,
    shell_twemoji_atlas_worklist_present_scanout, shell_twemoji_atlas_worklist_scanout,
    shell_twemoji_atlas_worklist_scanout_present, shell_twemoji_atlas_worklist_slot_scanout,
    sprite64_worklist_rgba8_upload_status, submit_alpha_blend_worklist_rgba8_probe_now,
    submit_fill_rect_worklist_rgba8_probe_now, submit_gradient_rect_worklist_rgba8_probe_now,
};
use crate::shell2::shell2_cmd::ParseOutcome;

const ATHLAS_GO_DEFAULT_DURATION_MS: u64 = 5_000;
const ATHLAS_GO_DEFAULT_CADENCE_MS: u64 = 0;
const ATHLAS_GO_DEFAULT_COUNT: u32 = 256;
const ATHLAS_GO_DEFAULT_PRESENT_EVERY: u32 = 1;
const ATHLAS_GO_MAX_COUNT: u32 = 256;
const ATHLAS_GO_MAX_PRESENT_EVERY: u32 = 1024;
const CANVAS_DEFAULT_DURATION_MS: u64 = 15_000;
const CANVAS_DEFAULT_CADENCE_US: u64 = 100_000;
const CANVAS_MIN_CADENCE_US: u64 = 100;
const CANVAS_MAX_CADENCE_US: u64 = 200_000;

static ATHLAS_GO_SEQUENCE: AtomicU32 = AtomicU32::new(0);

fn usage(io: &'static dyn ShellBackend2) {
    print_shell_line(io, "gpgpu status");
    print_shell_line(io, "gpgpu copy [sx sy dx dy w h]");
    print_shell_line(io, "gpgpu scanout");
    print_shell_line(io, "gpgpu atlas|athlas <id> [x,y]");
    print_shell_line(io, "gpgpu athlas work [count]");
    print_shell_line(io, "gpgpu athlas go [duration_ms] [cadence_ms] [count] [present_every]");
    print_shell_line(io, "gpgpu athlas_go [duration_ms] [cadence_ms] [count] [present_every]");
    print_shell_line(io, "gpgpu mandel [iterations]");
    print_shell_line(io, "gpgpu canvas [duration_ms] [cadence_ms:0.1..200]");
    print_shell_line(io, "gpgpu plane [half_q16]");
    print_shell_line(io, "gpgpu rectprobe");
    print_shell_line(io, "gpgpu smoke");
}

fn parse_i32(raw: Option<&str>) -> Option<i32> {
    raw?.parse::<i32>().ok()
}

fn parse_u32(raw: Option<&str>) -> Option<u32> {
    raw?.parse::<u32>().ok()
}

fn parse_slot_id(raw: Option<&str>) -> Option<u16> {
    let raw = raw?.trim_matches('"');
    if let Some(hex) = raw.strip_prefix("0x").or_else(|| raw.strip_prefix("0X")) {
        u16::from_str_radix(hex, 16).ok()
    } else {
        raw.parse::<u16>().ok()
    }
}

fn parse_copy_rect(args: &mut SplitWhitespace<'_>) -> Option<(GpgpuRect, GpgpuPoint)> {
    let Some(sx_raw) = args.next() else {
        return Some((GpgpuRect::new(0, 0, 32, 1), GpgpuPoint::new(32, 0)));
    };
    let sx = sx_raw.parse::<i32>().ok()?;
    let sy = parse_i32(args.next())?;
    let dx = parse_i32(args.next())?;
    let dy = parse_i32(args.next())?;
    let width = parse_u32(args.next())?;
    let height = parse_u32(args.next())?;
    if args.next().is_some() {
        return None;
    }
    Some((GpgpuRect::new(sx, sy, width, height), GpgpuPoint::new(dx, dy)))
}

fn parse_atlas_args(args: &mut SplitWhitespace<'_>) -> Option<(u16, Option<GpgpuPoint>)> {
    let slot = parse_slot_id(args.next())?;
    let Some(x_raw) = args.next() else {
        return Some((slot, None));
    };
    if let Some((x_part, y_part)) = x_raw.split_once(',') {
        let x = x_part.parse::<i32>().ok()?;
        let y = y_part.parse::<i32>().ok()?;
        if args.next().is_some() {
            return None;
        }
        return Some((slot, Some(GpgpuPoint::new(x, y))));
    }

    let x = x_raw.parse::<i32>().ok()?;
    let y = parse_i32(args.next())?;
    if args.next().is_some() {
        return None;
    }
    Some((slot, Some(GpgpuPoint::new(x, y))))
}

fn parse_atlas_go_args(args: &mut SplitWhitespace<'_>) -> Option<(u64, u64, u32, u32)> {
    let duration_ms = match args.next() {
        Some(raw) => raw.parse::<u64>().ok()?,
        None => ATHLAS_GO_DEFAULT_DURATION_MS,
    };
    let cadence_ms = match args.next() {
        Some(raw) => raw.parse::<u64>().ok()?,
        None => ATHLAS_GO_DEFAULT_CADENCE_MS,
    };
    let count = match args.next() {
        Some(raw) => raw.parse::<u32>().ok()?,
        None => ATHLAS_GO_DEFAULT_COUNT,
    }
    .clamp(1, ATHLAS_GO_MAX_COUNT);
    let present_every = match args.next() {
        Some(raw) => raw.parse::<u32>().ok()?,
        None => ATHLAS_GO_DEFAULT_PRESENT_EVERY,
    }
    .clamp(1, ATHLAS_GO_MAX_PRESENT_EVERY);
    if args.next().is_some() {
        return None;
    }
    Some((duration_ms, cadence_ms, count, present_every))
}

fn parse_atlas_work_args(args: &mut SplitWhitespace<'_>) -> Option<u32> {
    let count = match args.next() {
        Some(raw) => raw.parse::<u32>().ok()?,
        None => 16,
    };
    if args.next().is_some() {
        return None;
    }
    Some(count.clamp(1, 256))
}

fn parse_ms_text_to_us(raw: &str) -> Option<u64> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }

    let (whole_raw, frac_raw) = match raw.split_once('.') {
        Some((whole, frac)) => {
            if frac.contains('.') {
                return None;
            }
            (whole, Some(frac))
        }
        None => (raw, None),
    };
    if whole_raw.is_empty() && frac_raw.unwrap_or("").is_empty() {
        return None;
    }

    let whole_us = if whole_raw.is_empty() {
        0
    } else {
        whole_raw.parse::<u64>().ok()?.saturating_mul(1000)
    };
    let mut frac_us = 0u64;
    if let Some(frac) = frac_raw {
        let mut place_us = 100u64;
        for (index, byte) in frac.bytes().enumerate() {
            if !byte.is_ascii_digit() {
                return None;
            }
            if index < 3 {
                frac_us = frac_us.saturating_add(u64::from(byte - b'0').saturating_mul(place_us));
                place_us /= 10;
            }
        }
    }

    Some(whole_us.saturating_add(frac_us))
}

fn parse_canvas_args(args: &mut SplitWhitespace<'_>) -> Option<(u64, u64)> {
    let duration_ms = match args.next() {
        Some(raw) => raw.parse::<u64>().ok()?,
        None => CANVAS_DEFAULT_DURATION_MS,
    };
    let cadence_us = match args.next() {
        Some(raw) => parse_ms_text_to_us(raw)?.clamp(CANVAS_MIN_CADENCE_US, CANVAS_MAX_CADENCE_US),
        None => CANVAS_DEFAULT_CADENCE_US,
    };
    if args.next().is_some() {
        return None;
    }
    Some((duration_ms, cadence_us))
}

fn parse_plane_args(args: &mut SplitWhitespace<'_>) -> Option<i32> {
    let half_q16 = match args.next() {
        Some(raw) => raw.parse::<i32>().ok()?,
        None => 32_768,
    };
    if args.next().is_some() {
        return None;
    }
    Some(half_q16.clamp(1, 65_536 * 8))
}

fn hex4(values: [u32; 4]) -> AllocString {
    alloc::format!(
        "[0x{:08X},0x{:08X},0x{:08X},0x{:08X}]",
        values[0],
        values[1],
        values[2],
        values[3]
    )
}

fn vec3_text(values: [i32; 4]) -> AllocString {
    alloc::format!("[{},{},{},{}]", values[0], values[1], values[2], values[3])
}

fn artifact_status(uploaded: bool) -> u8 {
    uploaded as u8
}

fn now_ticks() -> u64 {
    embassy_time_driver::now()
}

fn ticks_from_ms(ms: u64) -> u64 {
    let hz = embassy_time_driver::TICK_HZ;
    if hz == 0 {
        return ms.max(1);
    }
    let ticks = ((ms as u128).saturating_mul(hz as u128).saturating_add(999) / 1000) as u64;
    if ms == 0 { 0 } else { ticks.max(1) }
}

fn elapsed_ms_since(start_tick: u64) -> u64 {
    let hz = embassy_time_driver::TICK_HZ;
    if hz == 0 {
        return 0;
    }
    now_ticks().saturating_sub(start_tick).saturating_mul(1000) / hz
}

fn wait_until_tick(deadline: u64) {
    while now_ticks() < deadline {
        core::hint::spin_loop();
    }
}

fn print_status(io: &'static dyn ShellBackend2) {
    let copy = copy_rect_rgba8_upload_status();
    let fill_worklist = fill_rect_worklist_rgba8_upload_status();
    let gradient_worklist = gradient_rect_worklist_rgba8_upload_status();
    let alpha_worklist = alpha_blend_worklist_rgba8_upload_status();
    let glyph = glyph_mask_rgba8_upload_status();
    let present = present_rgba8_to_primary_xrgb_rect_upload_status();
    let work = sprite64_worklist_rgba8_upload_status();
    let mandel64 = mandel64_worklist_rgba8_upload_status();
    let canvas = canvas3d_project_rgba8_upload_status();
    let transform = canvas3d_transform_q16_upload_status();
    let plane_fill = canvas3d_plane_fill_rgba8_upload_status();
    let msg = alloc::format!(
        "gpgpu: copy_upload={} fill_worklist_upload={} fill_worklist_ran={} fill_worklist_probe={} gradient_worklist_upload={} gradient_worklist_ran={} gradient_worklist_probe={} alpha_worklist_upload={} alpha_worklist_ran={} alpha_worklist_probe={} rect_worklist_ready={} glyph_mask_upload={} present_xrgb_upload={} worklist_upload={} mandel64_worklist_upload={} canvas3d_upload={} canvas3d_transform_upload={} canvas3d_plane_fill_upload={} shell_surface={}x{} pitch={} gpu=0x008A0000",
        artifact_status(copy.is_some()),
        artifact_status(fill_worklist.is_some()),
        artifact_status(fill_rect_worklist_probe_ran()),
        artifact_status(fill_rect_worklist_probe_ok()),
        artifact_status(gradient_worklist.is_some()),
        artifact_status(gradient_rect_worklist_probe_ran()),
        artifact_status(gradient_rect_worklist_probe_ok()),
        artifact_status(alpha_worklist.is_some()),
        artifact_status(alpha_blend_worklist_probe_ran()),
        artifact_status(alpha_blend_worklist_probe_ok()),
        artifact_status(rect_worklist_probe_ready()),
        artifact_status(glyph.is_some()),
        artifact_status(present.is_some()),
        artifact_status(work.is_some()),
        artifact_status(mandel64.is_some()),
        artifact_status(canvas.is_some()),
        artifact_status(transform.is_some()),
        artifact_status(plane_fill.is_some()),
        GPGPU_SHELL_SURFACE_WIDTH,
        GPGPU_SHELL_SURFACE_HEIGHT,
        GPGPU_SHELL_SURFACE_PITCH_BYTES,
    );
    print_shell_line(io, msg.as_str());
}

fn run_rect_probe(io: &'static dyn ShellBackend2, args: &mut SplitWhitespace<'_>) {
    if args.next().is_some() {
        usage(io);
        return;
    }

    let fill = submit_fill_rect_worklist_rgba8_probe_now();
    let gradient = submit_gradient_rect_worklist_rgba8_probe_now();
    let alpha = submit_alpha_blend_worklist_rgba8_probe_now();
    let msg = alloc::format!(
        "gpgpu rectprobe: fill={} gradient={} alpha={} fill_ran={} gradient_ran={} alpha_ran={} fill_ready={} gradient_ready={} alpha_ready={} ready={}",
        fill as u8,
        gradient as u8,
        alpha as u8,
        fill_rect_worklist_probe_ran() as u8,
        gradient_rect_worklist_probe_ran() as u8,
        alpha_blend_worklist_probe_ran() as u8,
        fill_rect_worklist_probe_ok() as u8,
        gradient_rect_worklist_probe_ok() as u8,
        alpha_blend_worklist_probe_ok() as u8,
        rect_worklist_probe_ready() as u8
    );
    print_shell_line(io, msg.as_str());
}

fn run_copy(io: &'static dyn ShellBackend2, args: &mut SplitWhitespace<'_>) {
    let Some((src_rect, dst_xy)) = parse_copy_rect(args) else {
        usage(io);
        return;
    };
    let Some(result) = shell_copy_rgba8(src_rect, dst_xy) else {
        print_shell_line(
            io,
            "gpgpu copy: no result (check iGPU claim, DMA, bounds, and non-overlap)",
        );
        return;
    };
    let src = hex4(result.src_head);
    let dst = hex4(result.dst_head);
    let msg = alloc::format!(
        "gpgpu copy: ok={} rect={}x{} src={},{} dst={},{} spans={}/{} submits={}/{} copied={}/{} src_preserved={}/{} gpu=0x{:X} phys=0x{:X} src_head={} dst_head={}",
        result.ok as u8,
        result.src_rect.width,
        result.src_rect.height,
        result.src_rect.x,
        result.src_rect.y,
        result.dst_xy.x,
        result.dst_xy.y,
        result.spans,
        result.expected_spans,
        result.submits,
        result.expected_submits,
        result.copied,
        result.pixels,
        result.src_preserved,
        result.pixels,
        result.surface.gpu,
        result.surface.phys,
        src,
        dst
    );
    print_shell_line(io, msg.as_str());
}

fn run_scanout(io: &'static dyn ShellBackend2, args: &mut SplitWhitespace<'_>) {
    if args.next().is_some() {
        usage(io);
        return;
    }
    let Some(result) = shell_copy_scanout_center_rgba8() else {
        print_shell_line(io, "gpgpu scanout: no result (check primary surface and iGPU claim)");
        return;
    };
    let src = hex4(result.src_head);
    let dst = hex4(result.dst_head);
    let msg = alloc::format!(
        "gpgpu scanout: ok={} rect={}x{} dst={},{} primary={}x{} pitch={} spans={}/{} submits={}/{} copied={}/{} src_preserved={}/{} presented={} primary_gpu=0x{:X} primary_phys=0x{:X} src_head={} dst_head={}",
        result.ok as u8,
        result.src_rect.width,
        result.src_rect.height,
        result.dst_xy.x,
        result.dst_xy.y,
        result.primary_width,
        result.primary_height,
        result.primary_pitch_bytes,
        result.spans,
        result.expected_spans,
        result.submits,
        result.expected_submits,
        result.copied,
        result.pixels,
        result.src_preserved,
        result.pixels,
        result.presented as u8,
        result.primary_gpu,
        result.primary_phys,
        src,
        dst
    );
    print_shell_line(io, msg.as_str());
}

fn run_atlas(io: &'static dyn ShellBackend2, args: &mut SplitWhitespace<'_>) {
    let mut lookahead = args.clone();
    if lookahead
        .next()
        .map(|raw| raw.eq_ignore_ascii_case("go"))
        .unwrap_or(false)
    {
        let _ = args.next();
        run_atlas_go(io, args);
        return;
    }
    if args
        .clone()
        .next()
        .map(|raw| raw.eq_ignore_ascii_case("work"))
        .unwrap_or(false)
    {
        let _ = args.next();
        run_atlas_work(io, args);
        return;
    }

    let Some((slot, dst_xy)) = parse_atlas_args(args) else {
        usage(io);
        return;
    };
    let Some(result) = shell_twemoji_atlas_worklist_slot_scanout(slot, dst_xy) else {
        print_shell_line(
            io,
            "gpgpu atlas: no result (check slot id, primary surface, destination bounds, and worklist artifact)",
        );
        return;
    };
    let msg = alloc::format!(
        "gpgpu athlas: mode=sprite64-worklist ok={} requested_id={} desc={} walkers={} pixels={} submit_ms={} present_ms={} total_ms={} id={} dst={},{} primary={}x{} slots={} atlas_gpu=0x{:X} desc_gpu=0x{:X} presented={}",
        result.ok as u8,
        slot,
        result.descriptors,
        result.walkers,
        result.copied_pixels,
        result.submit_ms,
        result.present_ms,
        result.total_ms,
        result.last_slot,
        result.last_dst_xy.x,
        result.last_dst_xy.y,
        result.primary_width,
        result.primary_height,
        result.slots,
        result.atlas_gpu,
        result.desc_gpu,
        result.presented as u8
    );
    print_shell_line(io, msg.as_str());
}

fn run_atlas_work(io: &'static dyn ShellBackend2, args: &mut SplitWhitespace<'_>) {
    let Some(count) = parse_atlas_work_args(args) else {
        usage(io);
        return;
    };
    let Some(result) = shell_twemoji_atlas_worklist_scanout(count) else {
        print_shell_line(
            io,
            "gpgpu athlas work: no result (check primary surface, atlas cache, and worklist artifact)",
        );
        return;
    };
    let msg = alloc::format!(
        "gpgpu athlas work: mode=sprite64-worklist ok={} requested={} desc={} walkers={} pixels={} submit_ms={} present_ms={} total_ms={} last_id={} last_dst={},{} primary={}x{} slots={} atlas_gpu=0x{:X} desc_gpu=0x{:X} presented={}",
        result.ok as u8,
        result.requested,
        result.descriptors,
        result.walkers,
        result.copied_pixels,
        result.submit_ms,
        result.present_ms,
        result.total_ms,
        result.last_slot,
        result.last_dst_xy.x,
        result.last_dst_xy.y,
        result.primary_width,
        result.primary_height,
        result.slots,
        result.atlas_gpu,
        result.desc_gpu,
        result.presented as u8
    );
    print_shell_line(io, msg.as_str());
}

fn run_mandel(io: &'static dyn ShellBackend2, args: &mut SplitWhitespace<'_>) {
    let first = args.next();
    let result = if let Some(raw) = first {
        if args.next().is_some() {
            usage(io);
            return;
        }
        let Some(iterations) = raw.parse::<u32>().ok() else {
            usage(io);
            return;
        };
        shell_mandel64_worklist_scanout(iterations.clamp(1, MANDEL64_WORKLIST_MAX_ITERATIONS))
    } else {
        shell_mandel64_worklist_scanout(MANDEL64_WORKLIST_DEFAULT_ITERATIONS)
    };

    let Some(result) = result else {
        print_shell_line(
            io,
            "gpgpu mandel: no result (check primary surface, iGPU claim, and mandel artifact)",
        );
        return;
    };
    let msg = alloc::format!(
        "gpgpu mandel: mode=mandel-worklist ok={} requested={} desc={} walkers={} pixels={} submit_ms={} present_ms={} total_ms={} last_src={},{} last_dst={},{} primary={}x{} desc_gpu=0x{:X} presented={}",
        result.ok as u8,
        result.requested,
        result.descriptors,
        result.walkers,
        result.pixels,
        result.submit_ms,
        result.present_ms,
        result.total_ms,
        result.last_src_xy.x,
        result.last_src_xy.y,
        result.last_dst_xy.x,
        result.last_dst_xy.y,
        result.primary_width,
        result.primary_height,
        result.desc_gpu,
        result.presented as u8
    );
    print_shell_line(io, msg.as_str());
}

fn run_atlas_go(io: &'static dyn ShellBackend2, args: &mut SplitWhitespace<'_>) {
    let Some((duration_ms, cadence_ms, count, present_every)) = parse_atlas_go_args(args) else {
        usage(io);
        return;
    };

    let start_tick = now_ticks();
    let deadline_tick = start_tick.saturating_add(ticks_from_ms(duration_ms));
    let cadence_ticks = ticks_from_ms(cadence_ms);
    let mut next_launch_tick = start_tick;
    let start_seq = ATHLAS_GO_SEQUENCE.load(Ordering::Relaxed);
    let mut ok_batches = 0u32;
    let mut fail_batches = 0u32;
    let mut fail_none = 0u32;
    let mut fail_not_ok = 0u32;
    let mut presented = 0u32;
    let mut final_presented = 0u32;
    let mut pending_present = 0u32;
    let mut measured = 0u32;
    let mut total_ms_sum = 0u64;
    let mut total_submit_ms = 0u64;
    let mut total_present_ms = 0u64;
    let mut max_total_ms = 0u64;
    let mut max_submit_ms = 0u64;
    let mut max_present_ms = 0u64;
    let mut total_desc = 0usize;
    let mut ok_desc = 0usize;
    let mut total_pixels = 0usize;
    let mut total_walkers = 0usize;
    let mut primary_width = 0u32;
    let mut primary_height = 0u32;
    let mut slots = 0u16;
    let mut last_slot = 0u16;
    let mut last_xy = GpgpuPoint::new(0, 0);

    while now_ticks() < deadline_tick {
        if cadence_ticks != 0 {
            wait_until_tick(next_launch_tick);
        }

        if now_ticks() >= deadline_tick {
            break;
        }

        let _seq = ATHLAS_GO_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let next_batch = ok_batches.saturating_add(fail_batches).saturating_add(1);
        let should_present = next_batch % present_every == 0;
        match shell_twemoji_atlas_worklist_scanout_present(count, should_present) {
            Some(result) => {
                measured = measured.saturating_add(1);
                primary_width = result.primary_width;
                primary_height = result.primary_height;
                slots = result.slots;
                last_slot = result.last_slot;
                last_xy = result.last_dst_xy;
                total_desc = total_desc.saturating_add(result.descriptors);
                total_walkers = total_walkers.saturating_add(result.walkers);
                total_pixels = total_pixels.saturating_add(result.copied_pixels);
                total_ms_sum = total_ms_sum.saturating_add(result.total_ms);
                total_submit_ms = total_submit_ms.saturating_add(result.submit_ms);
                total_present_ms = total_present_ms.saturating_add(result.present_ms);
                max_total_ms = max_total_ms.max(result.total_ms);
                max_submit_ms = max_submit_ms.max(result.submit_ms);
                max_present_ms = max_present_ms.max(result.present_ms);
                if result.presented {
                    presented = presented.saturating_add(1);
                    pending_present = 0;
                } else if result.submitted {
                    pending_present = pending_present.saturating_add(1);
                }
                if result.ok {
                    ok_batches = ok_batches.saturating_add(1);
                    ok_desc = ok_desc.saturating_add(result.descriptors);
                } else {
                    fail_batches = fail_batches.saturating_add(1);
                    fail_not_ok = fail_not_ok.saturating_add(1);
                }
            }
            None => {
                fail_batches = fail_batches.saturating_add(1);
                fail_none = fail_none.saturating_add(1);
            }
        }

        if cadence_ticks != 0 {
            next_launch_tick = next_launch_tick.saturating_add(cadence_ticks);
        }
    }

    if pending_present != 0 {
        if let Some(present_ms) = shell_twemoji_atlas_worklist_present_scanout() {
            final_presented = 1;
            presented = presented.saturating_add(1);
            total_present_ms = total_present_ms.saturating_add(present_ms);
            max_present_ms = max_present_ms.max(present_ms);
        }
    }

    let batches = ok_batches.saturating_add(fail_batches);
    let avg_total_ms = if measured == 0 {
        0
    } else {
        total_ms_sum / u64::from(measured)
    };
    let avg_submit_ms = if measured == 0 {
        0
    } else {
        total_submit_ms / u64::from(measured)
    };
    let avg_present_ms = if presented == 0 {
        0
    } else {
        total_present_ms / u64::from(presented)
    };
    let elapsed_ms = elapsed_ms_since(start_tick);
    let end_seq = ATHLAS_GO_SEQUENCE.load(Ordering::Relaxed);
    let msg = alloc::format!(
        "gpgpu athlas go: mode=sprite64-worklist batches={} ok={} fail={} fail_none={} fail_not_ok={} duration_ms={} elapsed_ms={} cadence_ms={} count={} present_every={} final_present={} seq={}..{} measured={} desc={} ok_desc={} walkers={} pixels={} presented={} avg_ms={} avg_submit_ms={} avg_present_ms={} max_ms={} max_submit_ms={} max_present_ms={} last_id={} last_dst={},{} primary={}x{} slots={}",
        batches,
        ok_batches,
        fail_batches,
        fail_none,
        fail_not_ok,
        duration_ms,
        elapsed_ms,
        cadence_ms,
        count,
        present_every,
        final_presented,
        start_seq,
        end_seq,
        measured,
        total_desc,
        ok_desc,
        total_walkers,
        total_pixels,
        presented,
        avg_total_ms,
        avg_submit_ms,
        avg_present_ms,
        max_total_ms,
        max_submit_ms,
        max_present_ms,
        last_slot,
        last_xy.x,
        last_xy.y,
        primary_width,
        primary_height,
        slots
    );
    print_shell_line(io, msg.as_str());
}

fn run_canvas(io: &'static dyn ShellBackend2, args: &mut SplitWhitespace<'_>) {
    let Some((duration_ms, cadence_us)) = parse_canvas_args(args) else {
        usage(io);
        return;
    };
    let Some(result) = shell_cube20_project_spin(duration_ms, cadence_us) else {
        print_shell_line(
            io,
            "gpgpu canvas: no result (check primary surface, iGPU claim, and canvas artifact)",
        );
        return;
    };
    let avg_submit_ms = if result.submitted == 0 {
        0
    } else {
        result.total_submit_ms / u64::from(result.submitted)
    };
    let msg = alloc::format!(
        "gpgpu canvas: mode=cube20-tetra10-transform-project-fullscreen ok={} frames={} submitted={} presented={} duration_ms={} elapsed_ms={} cadence_us={} avg_submit_ms={} max_submit_ms={} visible={} stamped={} vertices={} half_px={} canvas={},{} primary={}x{} last_angle={}",
        result.ok as u8,
        result.frames,
        result.submitted,
        result.presented,
        result.duration_ms,
        result.elapsed_ms,
        result.cadence_us,
        avg_submit_ms,
        result.max_submit_ms,
        result.visible_points,
        result.stamped_pixels,
        result.vertex_count,
        result.radius_px,
        result.canvas_xy.x,
        result.canvas_xy.y,
        result.primary_width,
        result.primary_height,
        result.last_angle_deg,
    );
    print_shell_line(io, msg.as_str());
}

fn run_plane(io: &'static dyn ShellBackend2, args: &mut SplitWhitespace<'_>) {
    let Some(h) = parse_plane_args(args) else {
        usage(io);
        return;
    };

    let q = 65_536;
    let constraints = [[q, 0, q, 0], [-q, 0, q, 0], [0, q, q, 0], [0, -q, q, 0]];
    let faces: [(&str, [i32; 4], [i32; 4], [i32; 4]); 6] = [
        ("front", [0, 0, h, 0], [h, 0, 0, 0], [0, h, 0, 0]),
        ("back", [0, 0, -h, 0], [-h, 0, 0, 0], [0, h, 0, 0]),
        ("right", [h, 0, 0, 0], [0, 0, -h, 0], [0, h, 0, 0]),
        ("left", [-h, 0, 0, 0], [0, 0, h, 0], [0, h, 0, 0]),
        ("top", [0, h, 0, 0], [h, 0, 0, 0], [0, 0, -h, 0]),
        ("bottom", [0, -h, 0, 0], [h, 0, 0, 0], [0, 0, h, 0]),
    ];

    let header = alloc::format!(
        "gpgpu plane: mode=cube6-face-math half_q16={} q16_one={} constraints c0={} c1={} c2={} c3={}",
        h,
        q,
        vec3_text(constraints[0]),
        vec3_text(constraints[1]),
        vec3_text(constraints[2]),
        vec3_text(constraints[3]),
    );
    print_shell_line(io, header.as_str());

    for (index, (name, origin, axis_u, axis_v)) in faces.iter().copied().enumerate() {
        let line = alloc::format!(
            "gpgpu plane face={} name={} origin={} axis_u={} axis_v={} local=P(origin+u*axis_u+v*axis_v) u,v=-1..1",
            index,
            name,
            vec3_text(origin),
            vec3_text(axis_u),
            vec3_text(axis_v),
        );
        print_shell_line(io, line.as_str());
    }
}

pub(crate) fn try_parse(
    io: &'static dyn ShellBackend2,
    args: &mut SplitWhitespace<'_>,
) -> ParseOutcome {
    let Some(cmd) = args.next() else {
        usage(io);
        return ParseOutcome::Handled;
    };

    if cmd.eq_ignore_ascii_case("status") {
        if args.next().is_some() {
            usage(io);
        } else {
            print_status(io);
        }
    } else if cmd.eq_ignore_ascii_case("copy") {
        run_copy(io, args);
    } else if cmd.eq_ignore_ascii_case("scanout") {
        run_scanout(io, args);
    } else if cmd.eq_ignore_ascii_case("atlas") || cmd.eq_ignore_ascii_case("athlas") {
        run_atlas(io, args);
    } else if cmd.eq_ignore_ascii_case("athlas_go") {
        run_atlas_go(io, args);
    } else if cmd.eq_ignore_ascii_case("mandel") {
        run_mandel(io, args);
    } else if cmd.eq_ignore_ascii_case("canvas") {
        run_canvas(io, args);
    } else if cmd.eq_ignore_ascii_case("plane") {
        run_plane(io, args);
    } else if cmd.eq_ignore_ascii_case("rectprobe") {
        run_rect_probe(io, args);
    } else if cmd.eq_ignore_ascii_case("smoke") {
        if args.next().is_some() {
            usage(io);
        } else {
            let mut probe_args = "".split_whitespace();
            run_rect_probe(io, &mut probe_args);
        }
    } else {
        usage(io);
    }

    ParseOutcome::Handled
}
