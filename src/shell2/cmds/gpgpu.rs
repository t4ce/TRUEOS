use alloc::string::String as AllocString;
use core::str::SplitWhitespace;
use core::sync::atomic::{AtomicU32, Ordering};

use super::super::{ShellBackend2, print_shell_line};
use crate::gfx::althlasfont::twemoji::twemoji_slot_count;
use crate::intel::gpgpu::{
    GPGPU_SHELL_SURFACE_HEIGHT, GPGPU_SHELL_SURFACE_PITCH_BYTES, GPGPU_SHELL_SURFACE_WIDTH,
    GpgpuPoint, GpgpuRect, clear_rect_rgba8_white_upload_status, copy_rect_rgba8_upload_status,
    copy_rect_rgba8_wide_upload_status, empty_eot_upload_status, shell_clear_white_rgba8,
    shell_copy_rgba8, shell_copy_scanout_center_rgba8, shell_copy_twemoji_atlas_slot_scanout,
    shell_copy_twemoji_atlas_slot_scanout_hot,
};
use crate::shell2::shell2_cmd::ParseOutcome;
use crate::tyche::SoftRng;

const ATHLAS_GO_DEFAULT_DURATION_MS: u64 = 5_000;
const ATHLAS_GO_DEFAULT_CADENCE_MS: u64 = 0;
const ATHLAS_GO_DEFAULT_BURST: u32 = 1;
const ATHLAS_GO_MAX_BURST: u32 = 8;
const ATHLAS_GO_TIMEOUTISH_COPY_MS: u64 = 1_000;
const ATHLAS_GO_FAIL_BACKOFF_MS: u64 = 3;

static ATHLAS_GO_SEQUENCE: AtomicU32 = AtomicU32::new(0);

fn usage(io: &'static dyn ShellBackend2) {
    print_shell_line(io, "gpgpu status");
    print_shell_line(io, "gpgpu clear [x y w h]");
    print_shell_line(io, "gpgpu copy [sx sy dx dy w h]");
    print_shell_line(io, "gpgpu scanout");
    print_shell_line(io, "gpgpu atlas|athlas <id> [x,y]");
    print_shell_line(io, "gpgpu athlas go [duration_ms] [cadence_ms] [burst]");
    print_shell_line(io, "gpgpu athlas_go [duration_ms] [cadence_ms] [burst]");
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

fn parse_clear_rect(args: &mut SplitWhitespace<'_>) -> Option<GpgpuRect> {
    let Some(x_raw) = args.next() else {
        return Some(GpgpuRect::new(0, 0, 4, 1));
    };
    let x = x_raw.parse::<i32>().ok()?;
    let y = parse_i32(args.next())?;
    let width = parse_u32(args.next())?;
    let height = parse_u32(args.next())?;
    if args.next().is_some() {
        return None;
    }
    Some(GpgpuRect::new(x, y, width, height))
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

fn parse_atlas_go_args(args: &mut SplitWhitespace<'_>) -> Option<(u64, u64, u32)> {
    let duration_ms = match args.next() {
        Some(raw) => raw.parse::<u64>().ok()?,
        None => ATHLAS_GO_DEFAULT_DURATION_MS,
    };
    let cadence_ms = match args.next() {
        Some(raw) => raw.parse::<u64>().ok()?,
        None => ATHLAS_GO_DEFAULT_CADENCE_MS,
    };
    let burst = match args.next() {
        Some(raw) => raw.parse::<u32>().ok()?,
        None => ATHLAS_GO_DEFAULT_BURST,
    }
    .clamp(1, ATHLAS_GO_MAX_BURST);
    if args.next().is_some() {
        return None;
    }
    Some((duration_ms, cadence_ms, burst))
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
    let copy_wide = copy_rect_rgba8_wide_upload_status();
    let clear = clear_rect_rgba8_white_upload_status();
    let empty = empty_eot_upload_status();
    let msg = alloc::format!(
        "gpgpu: copy_upload={} copy_wide_upload={} clear_upload={} empty_upload={} shell_surface={}x{} pitch={} gpu=0x008A0000",
        artifact_status(copy.is_some()),
        artifact_status(copy_wide.is_some()),
        artifact_status(clear.is_some()),
        artifact_status(empty.is_some()),
        GPGPU_SHELL_SURFACE_WIDTH,
        GPGPU_SHELL_SURFACE_HEIGHT,
        GPGPU_SHELL_SURFACE_PITCH_BYTES,
    );
    print_shell_line(io, msg.as_str());
}

fn run_clear(io: &'static dyn ShellBackend2, args: &mut SplitWhitespace<'_>) {
    let Some(rect) = parse_clear_rect(args) else {
        usage(io);
        return;
    };
    let Some(result) = shell_clear_white_rgba8(rect) else {
        print_shell_line(io, "gpgpu clear: no result (check iGPU claim, DMA, and rect bounds)");
        return;
    };
    let before = hex4(result.before_head);
    let after = hex4(result.after_head);
    let msg = alloc::format!(
        "gpgpu clear: ok={} rect={}x{}@{},{} spans={}/{} white={}/{} gpu=0x{:X} phys=0x{:X} before={} after={}",
        result.ok as u8,
        result.rect.width,
        result.rect.height,
        result.rect.x,
        result.rect.y,
        result.spans,
        result.expected_spans,
        result.white,
        result.pixels,
        result.surface.gpu,
        result.surface.phys,
        before,
        after
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

    let Some((slot, dst_xy)) = parse_atlas_args(args) else {
        usage(io);
        return;
    };
    let Some(result) = shell_copy_twemoji_atlas_slot_scanout(slot, dst_xy) else {
        print_shell_line(
            io,
            "gpgpu atlas: no result (check slot id, primary surface, and destination bounds)",
        );
        return;
    };
    let msg = alloc::format!(
        "gpgpu athlas: ok={} id={} sprite={}x{} dst={},{} ms={} stage_ms={} copy_ms={} submits={}/{} copied={}/{} presented={}",
        result.ok as u8,
        result.slot,
        result.atlas_src_rect.width,
        result.atlas_src_rect.height,
        result.dst_xy.x,
        result.dst_xy.y,
        result.total_ms,
        result.stage_ms,
        result.copy_ms,
        result.submits,
        result.expected_submits,
        result.copied,
        result.pixels,
        result.presented as u8
    );
    print_shell_line(io, msg.as_str());
}

fn athlas_go_slot(rng: &mut SoftRng, slot_count: u16) -> u16 {
    if slot_count == 0 {
        return 0;
    }
    rng.usize_below(slot_count as usize) as u16
}

fn athlas_go_point(
    rng: &mut SoftRng,
    primary_width: u32,
    primary_height: u32,
    sprite_width: u32,
    sprite_height: u32,
) -> Option<GpgpuPoint> {
    if primary_width == 0 || primary_height == 0 {
        return None;
    }
    let max_x = primary_width.saturating_sub(sprite_width).max(1);
    let max_y = primary_height.saturating_sub(sprite_height).max(1);
    let x = rng.usize_below(max_x as usize) as u32;
    let y = rng.usize_below(max_y as usize) as u32;
    Some(GpgpuPoint::new(x as i32, y as i32))
}

fn run_atlas_go(io: &'static dyn ShellBackend2, args: &mut SplitWhitespace<'_>) {
    let Some((duration_ms, cadence_ms, burst)) = parse_atlas_go_args(args) else {
        usage(io);
        return;
    };
    let slot_count = twemoji_slot_count();
    if slot_count == 0 {
        print_shell_line(io, "gpgpu athlas go: no twemoji slots");
        return;
    }

    let start_tick = now_ticks();
    let deadline_tick = start_tick.saturating_add(ticks_from_ms(duration_ms));
    let cadence_ticks = ticks_from_ms(cadence_ms);
    let mut next_launch_tick = start_tick;
    let start_seq = ATHLAS_GO_SEQUENCE.load(Ordering::Relaxed);
    let mut rng = crate::tyche::soft_rng();
    let mut ok = 0u32;
    let mut fail = 0u32;
    let mut fail_none = 0u32;
    let mut fail_not_ok = 0u32;
    let mut timeoutish = 0u32;
    let mut measured = 0u32;
    let mut total_ms_sum = 0u64;
    let mut total_stage_ms = 0u64;
    let mut total_copy_ms = 0u64;
    let mut max_total_ms = 0u64;
    let mut max_stage_ms = 0u64;
    let mut max_copy_ms = 0u64;
    let mut total_spans = 0usize;
    let mut total_submits = 0usize;
    let mut primary_width = 0u32;
    let mut primary_height = 0u32;
    let mut sprite_width = 32u32;
    let mut sprite_height = 32u32;
    let mut last_slot = 0u16;
    let mut last_xy = GpgpuPoint::new(0, 0);

    while now_ticks() < deadline_tick {
        if cadence_ticks != 0 {
            wait_until_tick(next_launch_tick);
        }

        for _ in 0..burst {
            if now_ticks() >= deadline_tick {
                break;
            }
            let _seq = ATHLAS_GO_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let slot = athlas_go_slot(&mut rng, slot_count);
            let dst_xy = athlas_go_point(
                &mut rng,
                primary_width,
                primary_height,
                sprite_width,
                sprite_height,
            );
            match shell_copy_twemoji_atlas_slot_scanout_hot(slot, dst_xy) {
                Some(result) => {
                    primary_width = result.primary_width;
                    primary_height = result.primary_height;
                    sprite_width = result.atlas_src_rect.width;
                    sprite_height = result.atlas_src_rect.height;
                    last_slot = result.slot;
                    last_xy = result.dst_xy;
                    measured = measured.saturating_add(1);
                    total_ms_sum = total_ms_sum.saturating_add(result.total_ms);
                    total_stage_ms = total_stage_ms.saturating_add(result.stage_ms);
                    total_copy_ms = total_copy_ms.saturating_add(result.copy_ms);
                    max_total_ms = max_total_ms.max(result.total_ms);
                    max_stage_ms = max_stage_ms.max(result.stage_ms);
                    max_copy_ms = max_copy_ms.max(result.copy_ms);
                    total_spans = total_spans.saturating_add(result.spans);
                    total_submits = total_submits.saturating_add(result.submits);
                    if result.ok {
                        ok = ok.saturating_add(1);
                    } else {
                        fail = fail.saturating_add(1);
                        fail_not_ok = fail_not_ok.saturating_add(1);
                        if result.copy_ms >= ATHLAS_GO_TIMEOUTISH_COPY_MS {
                            timeoutish = timeoutish.saturating_add(1);
                            wait_until_tick(
                                now_ticks()
                                    .saturating_add(ticks_from_ms(ATHLAS_GO_FAIL_BACKOFF_MS)),
                            );
                        }
                    }
                }
                None => {
                    fail = fail.saturating_add(1);
                    fail_none = fail_none.saturating_add(1);
                }
            }
        }

        if cadence_ticks != 0 {
            next_launch_tick = next_launch_tick.saturating_add(cadence_ticks);
        }
    }

    let copies = ok.saturating_add(fail);
    let avg_copy_ms = if copies == 0 {
        0
    } else {
        total_copy_ms / u64::from(measured.max(1))
    };
    let avg_stage_ms = if copies == 0 {
        0
    } else {
        total_stage_ms / u64::from(measured.max(1))
    };
    let avg_total_ms = if copies == 0 {
        0
    } else {
        total_ms_sum / u64::from(measured.max(1))
    };
    let elapsed_ms = elapsed_ms_since(start_tick);
    let end_seq = ATHLAS_GO_SEQUENCE.load(Ordering::Relaxed);
    let msg = alloc::format!(
        "gpgpu athlas go: copies={} ok={} fail={} fail_none={} fail_not_ok={} timeoutish={} duration_ms={} elapsed_ms={} cadence_ms={} burst={} fail_backoff_ms={} seq={}..{} measured={} spans={} submits={} avg_ms={} avg_stage_ms={} avg_copy_ms={} max_ms={} max_stage_ms={} max_copy_ms={} last_id={} last_dst={},{} slots={}",
        copies,
        ok,
        fail,
        fail_none,
        fail_not_ok,
        timeoutish,
        duration_ms,
        elapsed_ms,
        cadence_ms,
        burst,
        ATHLAS_GO_FAIL_BACKOFF_MS,
        start_seq,
        end_seq,
        measured,
        total_spans,
        total_submits,
        avg_total_ms,
        avg_stage_ms,
        avg_copy_ms,
        max_total_ms,
        max_stage_ms,
        max_copy_ms,
        last_slot,
        last_xy.x,
        last_xy.y,
        slot_count
    );
    print_shell_line(io, msg.as_str());
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
    } else if cmd.eq_ignore_ascii_case("clear") {
        run_clear(io, args);
    } else if cmd.eq_ignore_ascii_case("copy") {
        run_copy(io, args);
    } else if cmd.eq_ignore_ascii_case("scanout") {
        run_scanout(io, args);
    } else if cmd.eq_ignore_ascii_case("atlas") || cmd.eq_ignore_ascii_case("athlas") {
        run_atlas(io, args);
    } else if cmd.eq_ignore_ascii_case("athlas_go") {
        run_atlas_go(io, args);
    } else if cmd.eq_ignore_ascii_case("smoke") {
        if args.next().is_some() {
            usage(io);
        } else {
            let mut clear_args = "".split_whitespace();
            let mut copy_args = "".split_whitespace();
            run_clear(io, &mut clear_args);
            run_copy(io, &mut copy_args);
        }
    } else {
        usage(io);
    }

    ParseOutcome::Handled
}
