use alloc::string::String;
use core::fmt::Write;
use core::str::SplitWhitespace;
use core::sync::atomic::{AtomicU32, Ordering};

use embassy_executor::Spawner;

use super::super::{ShellBackend2, print_shell_line};
use crate::intel::gpgpu::{
    CHART_SINE_FLAG_AXES, CHART_SINE_FLAG_BORDER, CHART_SINE_FLAG_GLOW, CHART_SINE_FLAG_GRID,
    CHART_SINE_RGBA8_ADLS_ARTIFACT, GpgpuPoint, MANDEL64_WORKLIST_DEFAULT_ITERATIONS,
    MANDEL64_WORKLIST_MAX_ITERATIONS, reload_all_known_kernel_artifacts,
    reload_known_kernel_artifact, shell_chart_sine_scanout, shell_mandel64_worklist_scanout,
    shell_twemoji_atlas_worklist_present_scanout, shell_twemoji_atlas_worklist_scanout,
    shell_twemoji_atlas_worklist_scanout_present, upload_chart_sine_rgba8_kernel,
};
use crate::shell2::shell2_cmd::{CommandSessionKind, ParseOutcome};

const CANVAS2D_SPRITE_DEFAULT_DURATION_MS: u64 = 5_000;
const CANVAS2D_SPRITE_DEFAULT_CADENCE_MS: u64 = 0;
const CANVAS2D_SPRITE_DEFAULT_COUNT: u32 = 256;
const CANVAS2D_SPRITE_DEFAULT_PRESENT_EVERY: u32 = 1;
const CANVAS2D_SPRITE_MAX_COUNT: u32 = 256;
const CANVAS2D_SPRITE_MAX_PRESENT_EVERY: u32 = 1024;
const CANVAS2D_SPRITES64_COUNT: u32 = 16;
const CHART_WAVE_DEFAULT_DURATION_MS: u64 = 10_000;
const CHART_WAVE_DEFAULT_HZ: u32 = 60;
const CHART_WAVE_DEFAULT_PRESENT_EVERY: u32 = 1;
const CHART_WAVE_MAX_DURATION_MS: u64 = 120_000;
const CHART_WAVE_MAX_HZ: u32 = 240;
const CHART_ALL_FLAGS: u32 =
    CHART_SINE_FLAG_GRID | CHART_SINE_FLAG_AXES | CHART_SINE_FLAG_GLOW | CHART_SINE_FLAG_BORDER;

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
    print_shell_line(io, "gpgpu chart artifact");
    print_shell_line(io, "gpgpu chart static [phase]");
    print_shell_line(io, "gpgpu chart wave [duration_ms] [hz] [present_every]");
    print_shell_line(io, "gpgpu artifacts reload <kernel|all>");
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

fn elapsed_us_since(start_tick: u64) -> u64 {
    let hz = embassy_time_driver::TICK_HZ;
    if hz == 0 {
        return 0;
    }
    now_ticks()
        .saturating_sub(start_tick)
        .saturating_mul(1_000_000)
        / hz
}

fn ticks_for_hz(hz: u32) -> u64 {
    let clock_hz = embassy_time_driver::TICK_HZ;
    if clock_hz == 0 {
        return 1;
    }
    clock_hz.div_ceil(u64::from(hz.max(1))).max(1)
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

fn run_chart(io: &'static dyn ShellBackend2, args: &mut SplitWhitespace<'_>) {
    let Some(probe) = args.next() else {
        usage(io);
        return;
    };
    if probe.eq_ignore_ascii_case("artifact") {
        if expect_no_more(io, args) {
            run_chart_artifact(io);
        }
    } else if probe.eq_ignore_ascii_case("static") {
        run_chart_static(io, args);
    } else if probe.eq_ignore_ascii_case("wave") {
        run_chart_wave(io, args);
    } else {
        usage(io);
    }
}

fn run_chart_artifact(io: &'static dyn ShellBackend2) {
    let artifact = CHART_SINE_RGBA8_ADLS_ARTIFACT;
    let registry_report = crate::intel::opencl::trueos_cl_validate_known_aot_registry();
    let Some(known) = crate::intel::opencl::registry::known_aot_kernel(artifact.name) else {
        print_shell_line(
            io,
            "gpgpu chart artifact: ok=0 stage=registry reason=missing-known-aot-contract",
        );
        crate::log_error!(
            target: "gpgpu";
            "gpgpu chart artifact failed stage=registry kernel={}\n",
            artifact.name
        );
        return;
    };
    let Some(upload) = upload_chart_sine_rgba8_kernel() else {
        print_shell_line(io, "gpgpu chart artifact: ok=0 stage=upload reason=unavailable");
        return;
    };
    let hash_ok = upload.bin_sha256 == artifact.bin_sha256;
    let contract_ok = known.contract.name == artifact.name
        && known.contract.target == artifact.target
        && known.contract.cross_thread_bytes == 128
        && known.contract.per_thread_bytes == 96
        && known.contract.binding_count == 1;
    let ok =
        registry_report.passed() && upload.verified && upload.bytes != 0 && hash_ok && contract_ok;
    let message = alloc::format!(
        "gpgpu chart artifact: ok={} stage=artifact kernel={} role={:?} target={} producer={:?} source={} source_path={} bin_bytes=0x{:X} spv_bytes=0x{:X} gpu=0x{:X} mapped=0x{:X} verified={} hash_allowlisted={} contract_ok={} registry_ok={} registry_kernels={} registry_issues={} args={} bindings={} cross_thread={} per_thread={} sha256={}",
        ok as u8,
        artifact.name,
        known.role,
        artifact.target,
        known.contract.producer,
        upload.source,
        known.contract.source_path,
        artifact.bin.len(),
        artifact.spv.len(),
        upload.gpu,
        upload.mapped_bytes,
        upload.verified as u8,
        hash_ok as u8,
        contract_ok as u8,
        registry_report.passed() as u8,
        registry_report.registry_kernels,
        registry_report.issues,
        known.contract.args.len(),
        known.contract.binding_count,
        known.contract.cross_thread_bytes,
        known.contract.per_thread_bytes,
        digest_hex(&upload.bin_sha256),
    );
    print_shell_line(io, message.as_str());
    if ok {
        crate::log_info!(target: "gpgpu"; "{}\n", message.as_str());
    } else {
        crate::log_error!(target: "gpgpu"; "{}\n", message.as_str());
    }
}

fn run_chart_static(io: &'static dyn ShellBackend2, args: &mut SplitWhitespace<'_>) {
    let phase = match args.next() {
        Some(raw) => match raw.parse::<f32>() {
            Ok(value) if value.is_finite() => value,
            _ => {
                usage(io);
                return;
            }
        },
        None => 0.0,
    };
    if !expect_no_more(io, args) {
        return;
    }
    let flags = CHART_SINE_FLAG_GRID | CHART_SINE_FLAG_AXES | CHART_SINE_FLAG_BORDER;
    let Some(result) = shell_chart_sine_scanout(phase, flags, true) else {
        print_shell_line(io, "gpgpu chart static: ok=0 stage=dispatch reason=no-result");
        return;
    };
    let message = alloc::format!(
        "gpgpu chart static: ok={} stage=single-dispatch submitted={} presented={} size={}x{} pixels={} phase={:.4} flags=0x{:X} submit_us={} present_us={} total_us={} marker=0x{:08X}",
        result.ok as u8,
        result.submitted as u8,
        result.presented as u8,
        result.width,
        result.height,
        result.pixels,
        result.phase,
        flags,
        result.submit_us,
        result.present_us,
        result.total_us,
        result.marker,
    );
    print_shell_line(io, message.as_str());
    if result.ok {
        crate::log_info!(target: "gpgpu"; "{}\n", message.as_str());
    } else {
        crate::log_error!(target: "gpgpu"; "{}\n", message.as_str());
    }
}

fn run_chart_wave(io: &'static dyn ShellBackend2, args: &mut SplitWhitespace<'_>) {
    let duration_ms = match args.next() {
        Some(raw) => match raw.parse::<u64>() {
            Ok(value) => value.clamp(100, CHART_WAVE_MAX_DURATION_MS),
            Err(_) => {
                usage(io);
                return;
            }
        },
        None => CHART_WAVE_DEFAULT_DURATION_MS,
    };
    let hz = match args.next() {
        Some(raw) => match raw.parse::<u32>() {
            Ok(value) => value.clamp(1, CHART_WAVE_MAX_HZ),
            Err(_) => {
                usage(io);
                return;
            }
        },
        None => CHART_WAVE_DEFAULT_HZ,
    };
    let present_every = match args.next() {
        Some(raw) => match raw.parse::<u32>() {
            Ok(value) => value.clamp(1, 1024),
            Err(_) => {
                usage(io);
                return;
            }
        },
        None => CHART_WAVE_DEFAULT_PRESENT_EVERY,
    };
    if !expect_no_more(io, args) {
        return;
    }

    let start_tick = now_ticks();
    let deadline_tick = start_tick.saturating_add(ticks_from_ms(duration_ms));
    let cadence_ticks = ticks_for_hz(hz);
    let mut next_tick = start_tick;
    let mut frames = 0u64;
    let mut submitted = 0u64;
    let mut presented = 0u64;
    let mut failures = 0u64;
    let mut missed_deadlines = 0u64;
    let mut sum_submit_us = 0u64;
    let mut sum_present_us = 0u64;
    let mut max_submit_us = 0u64;
    let mut max_present_us = 0u64;
    let mut max_total_us = 0u64;
    let mut width = 0u32;
    let mut height = 0u32;
    let mut pending_present = false;
    let mut last_phase = 0.0f32;
    crate::log_info!(
        target: "gpgpu";
        "gpgpu chart wave begin duration_ms={} target_hz={} present_every={} flags=0x{:X}\n",
        duration_ms,
        hz,
        present_every,
        CHART_ALL_FLAGS
    );

    while now_ticks() < deadline_tick {
        wait_until_tick(next_tick);
        let now = now_ticks();
        if now >= deadline_tick {
            break;
        }
        let mut missed_this_frame = 0u64;
        if now > next_tick.saturating_add(cadence_ticks) {
            missed_this_frame = now.saturating_sub(next_tick) / cadence_ticks;
            missed_deadlines = missed_deadlines.saturating_add(missed_this_frame);
        }
        let elapsed_us = elapsed_us_since(start_tick);
        last_phase = elapsed_us as f32 * (6.2831855f32 * 0.35f32 / 1_000_000.0f32);
        let should_present = frames % u64::from(present_every) == 0;
        let Some(result) = shell_chart_sine_scanout(last_phase, CHART_ALL_FLAGS, should_present)
        else {
            failures = failures.saturating_add(1);
            break;
        };
        frames = frames.saturating_add(1);
        submitted = submitted.saturating_add(result.submitted as u64);
        presented = presented.saturating_add(result.presented as u64);
        failures = failures.saturating_add((!result.ok) as u64);
        pending_present = result.submitted && !result.presented;
        width = result.width;
        height = result.height;
        sum_submit_us = sum_submit_us.saturating_add(result.submit_us);
        sum_present_us = sum_present_us.saturating_add(result.present_us);
        max_submit_us = max_submit_us.max(result.submit_us);
        max_present_us = max_present_us.max(result.present_us);
        max_total_us = max_total_us.max(result.total_us);
        if !result.ok {
            break;
        }
        next_tick = next_tick
            .saturating_add(cadence_ticks.saturating_mul(missed_this_frame.saturating_add(1)));
    }

    let mut final_present = false;
    if pending_present
        && let Some(result) = shell_chart_sine_scanout(last_phase, CHART_ALL_FLAGS, true)
    {
        final_present = result.presented;
        presented = presented.saturating_add(result.presented as u64);
        failures = failures.saturating_add((!result.ok) as u64);
        sum_submit_us = sum_submit_us.saturating_add(result.submit_us);
        sum_present_us = sum_present_us.saturating_add(result.present_us);
        max_submit_us = max_submit_us.max(result.submit_us);
        max_present_us = max_present_us.max(result.present_us);
        max_total_us = max_total_us.max(result.total_us);
    }

    let elapsed_us = elapsed_us_since(start_tick).max(1);
    let fps_milli = frames.saturating_mul(1_000_000_000) / elapsed_us;
    let avg_submit_us = if frames == 0 {
        0
    } else {
        sum_submit_us / frames
    };
    let avg_present_us = if frames == 0 {
        0
    } else {
        sum_present_us / frames
    };
    let ok = frames != 0 && failures == 0 && submitted == frames;
    let message = alloc::format!(
        "gpgpu chart wave: ok={} stage=cadence frames={} submitted={} presented={} failures={} duration_ms={} elapsed_us={} target_hz={} fps={}.{:03} missed_deadlines={} present_every={} final_present={} size={}x{} avg_submit_us={} max_submit_us={} avg_present_us={} max_present_us={} max_total_us={} phase={:.4}",
        ok as u8,
        frames,
        submitted,
        presented,
        failures,
        duration_ms,
        elapsed_us,
        hz,
        fps_milli / 1000,
        fps_milli % 1000,
        missed_deadlines,
        present_every,
        final_present as u8,
        width,
        height,
        avg_submit_us,
        max_submit_us,
        avg_present_us,
        max_present_us,
        max_total_us,
        last_phase,
    );
    print_shell_line(io, message.as_str());
    if ok {
        crate::log_info!(target: "gpgpu"; "{}\n", message.as_str());
    } else {
        crate::log_error!(target: "gpgpu"; "{}\n", message.as_str());
    }
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

fn digest_hex(digest: &[u8; 32]) -> String {
    let mut out = String::with_capacity(64);
    for byte in digest {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn run_artifacts(io: &'static dyn ShellBackend2, args: &mut SplitWhitespace<'_>) {
    let Some(cmd) = args.next() else {
        usage(io);
        return;
    };
    if !cmd.eq_ignore_ascii_case("reload") {
        usage(io);
        return;
    }
    let Some(name) = args.next() else {
        usage(io);
        return;
    };
    if !expect_no_more(io, args) {
        return;
    }

    if name.eq_ignore_ascii_case("all") {
        let summary = reload_all_known_kernel_artifacts();
        print_shell_line(
            io,
            alloc::format!(
                "gpgpu artifacts reload all: attempted={} reloaded={} failed={}",
                summary.attempted,
                summary.reloaded,
                summary.failed
            )
            .as_str(),
        );
        return;
    }

    match reload_known_kernel_artifact(name) {
        Ok(upload) => print_shell_line(
            io,
            alloc::format!(
                "gpgpu artifacts reload {}: ok source={} gpu=0x{:X} bytes=0x{:X} sha256={}",
                upload.name,
                upload.source,
                upload.gpu,
                upload.bytes,
                digest_hex(&upload.bin_sha256)
            )
            .as_str(),
        ),
        Err(err) => print_shell_line(
            io,
            alloc::format!("gpgpu artifacts reload {}: failed reason={}", name, err.label())
                .as_str(),
        ),
    }
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
    } else if cmd.eq_ignore_ascii_case("chart") {
        run_chart(io, args);
    } else if cmd.eq_ignore_ascii_case("artifacts") {
        run_artifacts(io, args);
    } else if cmd.eq_ignore_ascii_case("smoke") {
        run_smoke(io, args);
    } else {
        usage(io);
    }

    ParseOutcome::Handled
}
