use core::str::SplitWhitespace;
use core::sync::atomic::{AtomicU32, Ordering};

use embassy_executor::Spawner;

use super::super::{ShellBackend2, print_shell_line};
use crate::intel::gpgpu::{
    GpgpuPoint, MANDEL64_WORKLIST_DEFAULT_ITERATIONS, MANDEL64_WORKLIST_MAX_ITERATIONS,
    shell_mandel64_worklist_scanout, shell_twemoji_atlas_worklist_present_scanout,
    shell_twemoji_atlas_worklist_scanout, shell_twemoji_atlas_worklist_scanout_present,
};
use crate::shell2::shell2_cmd::{CommandSessionKind, ParseOutcome};

const CANVAS2D_SPRITE_DEFAULT_DURATION_MS: u64 = 5_000;
const CANVAS2D_SPRITE_DEFAULT_CADENCE_MS: u64 = 0;
const CANVAS2D_SPRITE_DEFAULT_COUNT: u32 = 256;
const CANVAS2D_SPRITE_DEFAULT_PRESENT_EVERY: u32 = 1;
const CANVAS2D_SPRITE_MAX_COUNT: u32 = 256;
const CANVAS2D_SPRITE_MAX_PRESENT_EVERY: u32 = 1024;
const CANVAS2D_SPRITES64_COUNT: u32 = 16;

static CANVAS2D_SPRITE_SEQUENCE: AtomicU32 = AtomicU32::new(0);

fn usage(io: &'static dyn ShellBackend2) {
    print_shell_line(
        io,
        "gpgpu canvas2d sprite [duration_ms] [cadence_ms] [count] [present_every]",
    );
    print_shell_line(io, "gpgpu canvas2d sprites64");
    print_shell_line(io, "gpgpu canvas2d mandel64 [iterations]");
    print_shell_line(io, "gpgpu canvas3d cube");
    print_shell_line(io, "gpgpu canvas3d ico");
    print_shell_line(io, "gpgpu canvas3d para");
    print_shell_line(io, "gpgpu artificial-pixel");
    print_shell_line(io, "gpgpu smoke");
}

fn expect_no_more(io: &'static dyn ShellBackend2, args: &mut SplitWhitespace<'_>) -> bool {
    if args.next().is_none() {
        true
    } else {
        usage(io);
        false
    }
}

fn parse_canvas2d_sprite_args(args: &mut SplitWhitespace<'_>) -> Option<(u64, u64, u32, u32)> {
    let duration_ms = match args.next() {
        Some(raw) => raw.parse::<u64>().ok()?,
        None => CANVAS2D_SPRITE_DEFAULT_DURATION_MS,
    };
    let cadence_ms = match args.next() {
        Some(raw) => raw.parse::<u64>().ok()?,
        None => CANVAS2D_SPRITE_DEFAULT_CADENCE_MS,
    };
    let count = match args.next() {
        Some(raw) => raw.parse::<u32>().ok()?,
        None => CANVAS2D_SPRITE_DEFAULT_COUNT,
    }
    .clamp(1, CANVAS2D_SPRITE_MAX_COUNT);
    let present_every = match args.next() {
        Some(raw) => raw.parse::<u32>().ok()?,
        None => CANVAS2D_SPRITE_DEFAULT_PRESENT_EVERY,
    }
    .clamp(1, CANVAS2D_SPRITE_MAX_PRESENT_EVERY);
    if args.next().is_some() {
        return None;
    }
    Some((duration_ms, cadence_ms, count, present_every))
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

fn run_canvas2d(io: &'static dyn ShellBackend2, args: &mut SplitWhitespace<'_>) {
    let Some(kind) = args.next() else {
        usage(io);
        return;
    };

    if kind.eq_ignore_ascii_case("sprite") {
        run_canvas2d_sprite(io, args);
    } else if kind.eq_ignore_ascii_case("sprites64") {
        if !expect_no_more(io, args) {
            return;
        }
        run_canvas2d_sprites64(io);
    } else if kind.eq_ignore_ascii_case("mandel64") {
        run_canvas2d_mandel64(io, args);
    } else {
        usage(io);
    }
}

fn run_canvas2d_sprite(io: &'static dyn ShellBackend2, args: &mut SplitWhitespace<'_>) {
    let Some((duration_ms, cadence_ms, count, present_every)) = parse_canvas2d_sprite_args(args)
    else {
        usage(io);
        return;
    };

    let start_tick = now_ticks();
    let deadline_tick = start_tick.saturating_add(ticks_from_ms(duration_ms));
    let cadence_ticks = ticks_from_ms(cadence_ms);
    let mut next_launch_tick = start_tick;
    let start_seq = CANVAS2D_SPRITE_SEQUENCE.load(Ordering::Relaxed);
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

        let _seq = CANVAS2D_SPRITE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
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
    let end_seq = CANVAS2D_SPRITE_SEQUENCE.load(Ordering::Relaxed);
    let msg = alloc::format!(
        "gpgpu canvas2d sprite: mode=sprite64-worklist batches={} ok={} fail={} fail_none={} fail_not_ok={} duration_ms={} elapsed_ms={} cadence_ms={} count={} present_every={} final_present={} seq={}..{} measured={} desc={} ok_desc={} walkers={} pixels={} presented={} avg_ms={} avg_submit_ms={} avg_present_ms={} max_ms={} max_submit_ms={} max_present_ms={} last_id={} last_dst={},{} primary={}x{} slots={}",
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

fn run_canvas2d_sprites64(io: &'static dyn ShellBackend2) -> bool {
    let Some(result) = shell_twemoji_atlas_worklist_scanout(CANVAS2D_SPRITES64_COUNT) else {
        print_shell_line(
            io,
            "gpgpu canvas2d sprites64: no result (check primary surface, atlas cache, and worklist artifact)",
        );
        return false;
    };
    let msg = alloc::format!(
        "gpgpu canvas2d sprites64: mode=sprite64-worklist ok={} requested={} desc={} walkers={} pixels={} submit_ms={} present_ms={} total_ms={} last_id={} last_dst={},{} primary={}x{} slots={} atlas_gpu=0x{:X} desc_gpu=0x{:X} presented={}",
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
    result.ok
}

fn run_canvas2d_mandel64(io: &'static dyn ShellBackend2, args: &mut SplitWhitespace<'_>) -> bool {
    let iterations = match args.next() {
        Some(value) => match value.parse::<u32>() {
            Ok(iterations) => iterations.clamp(1, MANDEL64_WORKLIST_MAX_ITERATIONS),
            Err(_) => {
                usage(io);
                return false;
            }
        },
        None => MANDEL64_WORKLIST_DEFAULT_ITERATIONS,
    };
    if !expect_no_more(io, args) {
        return false;
    }

    let Some(result) = shell_mandel64_worklist_scanout(iterations) else {
        print_shell_line(
            io,
            "gpgpu canvas2d mandel64: no result (check primary surface, iGPU claim, and mandel artifact)",
        );
        return false;
    };
    let msg = alloc::format!(
        "gpgpu canvas2d mandel64: mode=mandel64-worklist ok={} iterations={} requested={} desc={} walkers={} pixels={} submit_ms={} present_ms={} total_ms={} last_src={},{} last_dst={},{} primary={}x{} desc_gpu=0x{:X} presented={}",
        result.ok as u8,
        iterations,
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
    result.ok
}

fn run_canvas3d(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    args: &mut SplitWhitespace<'_>,
) -> ParseOutcome {
    let Some(kind) = args.next() else {
        usage(io);
        return ParseOutcome::Handled;
    };
    if !expect_no_more(io, args) {
        return ParseOutcome::Handled;
    }

    let session_id = if kind.eq_ignore_ascii_case("cube") {
        crate::ui3::ui3_canvas::submit_canvas3d_cube(spawner, io)
    } else if kind.eq_ignore_ascii_case("ico") {
        crate::ui3::ui3_canvas::submit_canvas3d_ico(spawner, io)
    } else if kind.eq_ignore_ascii_case("para") {
        crate::ui3::ui3_canvas::submit_canvas3d_para(spawner, io)
    } else {
        usage(io);
        return ParseOutcome::Handled;
    };

    match session_id {
        Some(session_id) => {
            ParseOutcome::StartSession(CommandSessionKind::GpuCanvasRunning(session_id))
        }
        None => ParseOutcome::Handled,
    }
}

fn run_smoke(io: &'static dyn ShellBackend2, args: &mut SplitWhitespace<'_>) {
    if !expect_no_more(io, args) {
        return;
    }
    let sprites_ok = run_canvas2d_sprites64(io);
    let mut mandel_args = "".split_whitespace();
    let mandel_ok = run_canvas2d_mandel64(io, &mut mandel_args);
    let msg = alloc::format!(
        "gpgpu smoke: canvas2d_sprites64={} canvas2d_mandel64={} ok={}",
        sprites_ok as u8,
        mandel_ok as u8,
        (sprites_ok && mandel_ok) as u8
    );
    print_shell_line(io, msg.as_str());
}

fn run_artificial_pixel(io: &'static dyn ShellBackend2, args: &mut SplitWhitespace<'_>) {
    if !expect_no_more(io, args) {
        return;
    }

    let Some(result) = shell_mandel64_worklist_scanout(MANDEL64_WORKLIST_DEFAULT_ITERATIONS) else {
        print_shell_line(
            io,
            "gpgpu artificial-pixel: no result (check primary surface, iGPU claim, and mandel artifact)",
        );
        return;
    };
    let msg = alloc::format!(
        "gpgpu artificial-pixel: mode=mandel64-worklist ok={} desc={} walkers={} pixels={} submit_ms={} present_ms={} presented={} meaning=compute-driven-pixels-not-wm",
        result.ok as u8,
        result.descriptors,
        result.walkers,
        result.pixels,
        result.submit_ms,
        result.present_ms,
        result.presented as u8
    );
    print_shell_line(io, msg.as_str());
}

pub(crate) fn try_parse(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    args: &mut SplitWhitespace<'_>,
) -> ParseOutcome {
    let Some(cmd) = args.next() else {
        usage(io);
        return ParseOutcome::Handled;
    };

    if cmd.eq_ignore_ascii_case("canvas2d") {
        run_canvas2d(io, args);
    } else if cmd.eq_ignore_ascii_case("canvas3d") {
        return run_canvas3d(spawner, io, args);
    } else if cmd.eq_ignore_ascii_case("artificial-pixel") {
        run_artificial_pixel(io, args);
    } else if cmd.eq_ignore_ascii_case("smoke") {
        run_smoke(io, args);
    } else {
        usage(io);
    }

    ParseOutcome::Handled
}
