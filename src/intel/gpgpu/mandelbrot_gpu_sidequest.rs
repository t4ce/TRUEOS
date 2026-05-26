#![allow(dead_code)]

use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use embassy_executor::{SpawnError, Spawner};
use embassy_time::{Duration as EmbassyDuration, Timer};

pub(crate) const MANDELBROT_GPU_SIDEQUEST_NAME: &str = "mandelbrot-gpu-sidequest";
pub(crate) const MANDELBROT_TARGET_WIDTH: u32 = 2560;
pub(crate) const MANDELBROT_TARGET_HEIGHT: u32 = 1440;
pub(crate) const MANDELBROT_GPGPU_LOOP_MS: u64 = 16;
pub(crate) const MANDELBROT_GPGPU_RGB_ZOOM_RECT_WIDTH: u64 = 1280;
pub(crate) const MANDELBROT_GPGPU_RGB_ZOOM_RECT_HEIGHT: u64 = MANDELBROT_TARGET_HEIGHT as u64;
pub(crate) const MANDELBROT_GPGPU_ANIM_BAND_HEIGHT: u64 = 8;
pub(crate) const MANDELBROT_GPGPU_ANIM_PHASE_ROWS_PER_FRAME: u64 =
    MANDELBROT_GPGPU_ANIM_BAND_HEIGHT;
pub(crate) const MANDELBROT_GPGPU_FULL_FRAME_COLOR_FLIP: bool = true;
pub(crate) const MANDELBROT_GPGPU_GROUPID_LINE1280_ROWS_PER_BURST: u64 = 128;
// One 256+ row submit dispatches partway but misses the post-walker marker.
// Keep the visible area larger, but split it into smaller row-group walkers.
pub(crate) const MANDELBROT_GPGPU_GROUPID_VISUAL_ONLY_IGNORE_RETIRE: bool = true;
pub(crate) const MANDELBROT_GPGPU_LINE1280_MAX_SEGMENTS_PER_BURST: u64 =
    MANDELBROT_TARGET_HEIGHT as u64;
pub(crate) const MANDELBROT_GPGPU_PRESENT_FLUSH_BYTES: usize = 0xE10000;
pub(crate) const MANDELBROT_GPGPU_ANIM_PALETTE: [u32; 8] = [
    0x0000_0000,
    0x00FF_FFFF,
    0x00FF_00FF,
    0x0000_FFFF,
    0x00FF_FF00,
    0x0000_88FF,
    0x0088_FF00,
    0x00FF_8800,
];
pub(crate) const MANDELBROT_GPGPU_NOTIFY_AFTER_FULLSCREEN_SWEEP: bool = true;
pub(crate) const MANDELBROT_GPGPU_RUN_Q12_SIMD8X4_PREVIEW: bool = false;
pub(crate) const MANDELBROT_GPGPU_RUN_MANDELBROT16_SIMD16_STORE_PROBE: bool = true;
pub(crate) const MANDELBROT_GPGPU_RUN_LEGACY_ROW_WRITER_FALLBACK_ON_SUCCESS: bool = false;
pub(crate) const MANDELBROT_GPGPU_RUN_LEGACY_ROW_WRITER_FALLBACK_ON_FAILURE: bool = false;
pub(crate) const MANDELBROT_GPGPU_STAGE_READY_HEARTBEAT_MS: u64 = 1_000;
pub(crate) const MANDELBROT_BURST_LANE_POOL_START: u32 = 3;
pub(crate) const MANDELBROT_BURST_LANE_POOL_SIZE: u32 = 4;
pub(crate) const MANDELBROT_GPGPU_BURSTS_PER_FRAME_BUDGET: u64 = 2;
pub(crate) const MANDELBROT_ARTIFACT_FLAG_FULL_FRAME_COLOR_FLIP: u32 = 1 << 0;
pub(crate) const MANDELBROT_ARTIFACT_FLAG_VISUAL_ONLY_IGNORE_RETIRE: u32 = 1 << 1;
pub(crate) const MANDELBROT_ARTIFACT_FLAG_NOTIFY_AFTER_SWEEP: u32 = 1 << 2;
pub(crate) const MANDELBROT_ARTIFACT_KNOWN_FLAGS: u32 =
    MANDELBROT_ARTIFACT_FLAG_FULL_FRAME_COLOR_FLIP
        | MANDELBROT_ARTIFACT_FLAG_VISUAL_ONLY_IGNORE_RETIRE
        | MANDELBROT_ARTIFACT_FLAG_NOTIFY_AFTER_SWEEP;
pub(crate) const MANDELBROT_ARTIFACT_DEFAULT_FLAGS: u32 =
    MANDELBROT_ARTIFACT_FLAG_FULL_FRAME_COLOR_FLIP
        | MANDELBROT_ARTIFACT_FLAG_VISUAL_ONLY_IGNORE_RETIRE
        | MANDELBROT_ARTIFACT_FLAG_NOTIFY_AFTER_SWEEP;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct MandelbrotArtifactControlSnapshot {
    pub(crate) version: u64,
    pub(crate) flags: u32,
    pub(crate) rect_height_rows: u32,
    pub(crate) row_groups_per_burst: u32,
    pub(crate) bursts_per_frame_budget: u32,
    pub(crate) phase_rows_per_sweep: u32,
}

impl MandelbrotArtifactControlSnapshot {
    pub(crate) const fn default_runtime() -> Self {
        Self {
            version: 0,
            flags: MANDELBROT_ARTIFACT_DEFAULT_FLAGS,
            rect_height_rows: MANDELBROT_GPGPU_RGB_ZOOM_RECT_HEIGHT as u32,
            row_groups_per_burst: MANDELBROT_GPGPU_GROUPID_LINE1280_ROWS_PER_BURST as u32,
            bursts_per_frame_budget: MANDELBROT_GPGPU_BURSTS_PER_FRAME_BUDGET as u32,
            phase_rows_per_sweep: MANDELBROT_GPGPU_ANIM_PHASE_ROWS_PER_FRAME as u32,
        }
    }

    pub(crate) const fn full_frame_color_flip(self) -> bool {
        self.flags & MANDELBROT_ARTIFACT_FLAG_FULL_FRAME_COLOR_FLIP != 0
    }

    pub(crate) const fn visual_only_ignore_retire(self) -> bool {
        self.flags & MANDELBROT_ARTIFACT_FLAG_VISUAL_ONLY_IGNORE_RETIRE != 0
    }

    pub(crate) const fn notify_after_sweep(self) -> bool {
        self.flags & MANDELBROT_ARTIFACT_FLAG_NOTIFY_AFTER_SWEEP != 0
    }
}

static MANDELBROT_ARTIFACT_CONTROL_VERSION: AtomicU64 = AtomicU64::new(0);
static MANDELBROT_ARTIFACT_CONTROL_FLAGS: AtomicU32 =
    AtomicU32::new(MANDELBROT_ARTIFACT_DEFAULT_FLAGS);
static MANDELBROT_ARTIFACT_CONTROL_RECT_HEIGHT_ROWS: AtomicU32 =
    AtomicU32::new(MANDELBROT_GPGPU_RGB_ZOOM_RECT_HEIGHT as u32);
static MANDELBROT_ARTIFACT_CONTROL_ROW_GROUPS_PER_BURST: AtomicU32 =
    AtomicU32::new(MANDELBROT_GPGPU_GROUPID_LINE1280_ROWS_PER_BURST as u32);
static MANDELBROT_ARTIFACT_CONTROL_BURSTS_PER_FRAME_BUDGET: AtomicU32 =
    AtomicU32::new(MANDELBROT_GPGPU_BURSTS_PER_FRAME_BUDGET as u32);
static MANDELBROT_ARTIFACT_CONTROL_PHASE_ROWS_PER_SWEEP: AtomicU32 =
    AtomicU32::new(MANDELBROT_GPGPU_ANIM_PHASE_ROWS_PER_FRAME as u32);

/// Per-burst slot assignment for row-group walker distribution across CPU cores.
/// Minimal thread coordination: round-robin bursts within a stable slot pool.
static BURST_SLOT_COORDINATOR: AtomicU32 = AtomicU32::new(0);

pub(crate) fn mandelbrot_artifact_control_snapshot() -> MandelbrotArtifactControlSnapshot {
    MandelbrotArtifactControlSnapshot {
        version: MANDELBROT_ARTIFACT_CONTROL_VERSION.load(Ordering::Acquire),
        flags: MANDELBROT_ARTIFACT_CONTROL_FLAGS.load(Ordering::Acquire),
        rect_height_rows: MANDELBROT_ARTIFACT_CONTROL_RECT_HEIGHT_ROWS.load(Ordering::Acquire),
        row_groups_per_burst: MANDELBROT_ARTIFACT_CONTROL_ROW_GROUPS_PER_BURST
            .load(Ordering::Acquire),
        bursts_per_frame_budget: MANDELBROT_ARTIFACT_CONTROL_BURSTS_PER_FRAME_BUDGET
            .load(Ordering::Acquire),
        phase_rows_per_sweep: MANDELBROT_ARTIFACT_CONTROL_PHASE_ROWS_PER_SWEEP
            .load(Ordering::Acquire),
    }
}

pub(crate) fn mandelbrot_artifact_control_replace(
    mut next: MandelbrotArtifactControlSnapshot,
) -> MandelbrotArtifactControlSnapshot {
    next.flags &= MANDELBROT_ARTIFACT_KNOWN_FLAGS;
    next.rect_height_rows = next.rect_height_rows.clamp(1, MANDELBROT_TARGET_HEIGHT);
    next.row_groups_per_burst = next.row_groups_per_burst.clamp(1, MANDELBROT_TARGET_HEIGHT);
    next.bursts_per_frame_budget = next
        .bursts_per_frame_budget
        .clamp(1, MANDELBROT_TARGET_HEIGHT);
    next.phase_rows_per_sweep = next.phase_rows_per_sweep.clamp(1, MANDELBROT_TARGET_HEIGHT);

    MANDELBROT_ARTIFACT_CONTROL_FLAGS.store(next.flags, Ordering::Release);
    MANDELBROT_ARTIFACT_CONTROL_RECT_HEIGHT_ROWS.store(next.rect_height_rows, Ordering::Release);
    MANDELBROT_ARTIFACT_CONTROL_ROW_GROUPS_PER_BURST
        .store(next.row_groups_per_burst, Ordering::Release);
    MANDELBROT_ARTIFACT_CONTROL_BURSTS_PER_FRAME_BUDGET
        .store(next.bursts_per_frame_budget, Ordering::Release);
    MANDELBROT_ARTIFACT_CONTROL_PHASE_ROWS_PER_SWEEP
        .store(next.phase_rows_per_sweep, Ordering::Release);
    next.version = MANDELBROT_ARTIFACT_CONTROL_VERSION.fetch_add(1, Ordering::AcqRel) + 1;
    next
}

fn assign_burst_slot() -> u32 {
    let all_slots = crate::workers::background_worker_slots();
    if all_slots.is_empty() {
        return 0; // fallback to any available slot
    }
    // Use stable pool starting at MANDELBROT_BURST_LANE_POOL_START
    let _pool_start = MANDELBROT_BURST_LANE_POOL_START as usize;
    let pool_size = MANDELBROT_BURST_LANE_POOL_SIZE as usize;

    // Find how many slots we can actually use from our pool
    let available_from_pool = all_slots
        .iter()
        .filter(|&&s| s >= MANDELBROT_BURST_LANE_POOL_START)
        .count()
        .min(pool_size);

    if available_from_pool == 0 {
        // Fallback: use first available background slot
        return all_slots[0];
    }

    let burst_counter = BURST_SLOT_COORDINATOR.fetch_add(1, Ordering::Relaxed);
    let offset = (burst_counter as usize) % available_from_pool;
    let mut count = 0;
    for &slot in &all_slots {
        if slot >= MANDELBROT_BURST_LANE_POOL_START {
            if count == offset {
                return slot;
            }
            count += 1;
        }
    }
    all_slots[0] // fallback
}

pub(crate) fn spawn_mandelbrot_gpu_sidequest(spawner: Spawner) -> Result<(), SpawnError> {
    let token = mandelbrot_gpu_sidequest_task()?;
    spawner.spawn(token);
    Ok(())
}

#[embassy_executor::task(pool_size = 1)]
pub(crate) async fn mandelbrot_gpu_sidequest_task() {
    let scanout = crate::intel::active_scanout_dimensions();
    let scanout_w = scanout.map(|dims| dims.0).unwrap_or(0);
    let scanout_h = scanout.map(|dims| dims.1).unwrap_or(0);
    let primary_gpu = crate::intel::primary_surface_gpu_addr().unwrap_or(0);

    crate::log!(
        "mandelbrot-gpu-sidequest: attempted name={} called=1 hot=1 artifact_stage=gpgpu-eu target={}x{} scanout={}x{} primary_gpu=0x{:X} simd16_store_probe_enabled={} q12_simd8x4_reference_enabled={} legacy_row_fallback_on_success={} legacy_row_fallback_on_failure={} action=boot-exercise-simd16-mul-dword-witness-store cpu_runtime_patches=address-only eu_runtime_work=simd16-mul-dword-three-by-three-store-eot next=restore-fixed10-q12-body-after-mul-witness does_not_prove=full-frame-mandelbrot\n",
        MANDELBROT_GPU_SIDEQUEST_NAME,
        MANDELBROT_TARGET_WIDTH,
        MANDELBROT_TARGET_HEIGHT,
        scanout_w,
        scanout_h,
        primary_gpu,
        MANDELBROT_GPGPU_RUN_MANDELBROT16_SIMD16_STORE_PROBE as u8,
        MANDELBROT_GPGPU_RUN_Q12_SIMD8X4_PREVIEW as u8,
        MANDELBROT_GPGPU_RUN_LEGACY_ROW_WRITER_FALLBACK_ON_SUCCESS as u8,
        MANDELBROT_GPGPU_RUN_LEGACY_ROW_WRITER_FALLBACK_ON_FAILURE as u8,
    );

    if MANDELBROT_GPGPU_RUN_Q12_SIMD8X4_PREVIEW {
        let mut frame: u64 = 0;
        let mut cursor = 0usize;
        let mut released_lumen = false;
        loop {
            let control = mandelbrot_artifact_control_snapshot();
            let pixel_budget = (control.row_groups_per_burst as usize)
                .saturating_mul(
                    trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_PIXELS_PER_PROGRAM,
                )
                .max(trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_PIXELS_PER_PROGRAM);
            let (proof, next_cursor) =
                crate::intel::submit_gpgpu_primary_scanout_mandelbrot_preview(
                    cursor,
                    frame as usize,
                    pixel_budget,
                );
            cursor = next_cursor;
            if proof.submitted && !released_lumen {
                crate::r::readiness::set(crate::r::readiness::MANDELBROT_GPU_SIDEQUEST_READY);
                released_lumen = true;
            }
            let missing_hardware_or_scanout = !proof.submitted
                && (proof.reason == "no-device"
                    || proof.reason == "no-warm-state"
                    || proof.reason == "no-primary-scanout");
            let should_log = frame < 4 || frame % 64 == 0 || !proof.readback_ok;
            if should_log {
                crate::log!(
                    "mandelbrot-gpu-sidequest: q12-preview frame={} cursor={} submitted={} finished={} readback_ok={} reason={} program_source={} target_gpu=0x{:X} expected_mask=0x{:016X} lane_dispatch_delta={} finish_marker=0x{:08X} lumen_released={} cpu_runtime_patches=coords-and-address-bases eu_runtime_work=q12-iteration-color-and-hdc-message-payload message_contract=4x-hdc-simd8-store next={} does_not_prove=simd16-register-pair-body\n",
                    frame,
                    cursor,
                    proof.submitted as u8,
                    proof.finished as u8,
                    proof.readback_ok as u8,
                    proof.reason,
                    proof.program_name,
                    proof.output_gpu,
                    proof.output_hits_lo64,
                    proof.dispatch_delta,
                    proof.finish_marker,
                    released_lumen as u8,
                    if proof.readback_ok {
                        "continue-q12-preview"
                    } else if proof.dispatch_delta != 0 {
                        "fix-q12-readback-or-store-payload"
                    } else {
                        "fix-q12-submit-or-walker"
                    },
                );
            }
            if missing_hardware_or_scanout {
                loop {
                    Timer::after(EmbassyDuration::from_millis(
                        MANDELBROT_GPGPU_STAGE_READY_HEARTBEAT_MS,
                    ))
                    .await;
                    frame = frame.wrapping_add(1);
                    if frame == 1 || frame % 16 == 0 {
                        crate::log!(
                            "mandelbrot-gpu-sidequest: q12-preview-parked heartbeat={} reason={} submitted=0 frame_loop_running=0 next=boot-on-intel-gpgpu-hardware does_not_prove=mandelbrot-iteration-math\n",
                            frame,
                            proof.reason,
                        );
                    }
                }
            }
            frame = frame.wrapping_add(1);
            Timer::after(EmbassyDuration::from_millis(MANDELBROT_GPGPU_LOOP_MS)).await;
        }
    }

    let mut frame: u64 = 0;
    let mut released_lumen = false;
    let mut mandelbrot16_probe_readback_ok = false;
    let mut mandelbrot16_probe_finished = false;
    if MANDELBROT_GPGPU_RUN_MANDELBROT16_SIMD16_STORE_PROBE {
        let x_base = ((scanout_w as u32).saturating_sub(
            trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_PIXELS_PER_PROGRAM as u32,
        )) / 2;
        let row_index = ((scanout_h as u32) / 2).saturating_sub(16);
        let probe = crate::intel::submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_probe(
            1, row_index, x_base,
        );
        if probe.readback_ok && !released_lumen {
            crate::r::readiness::set(crate::r::readiness::MANDELBROT_GPU_SIDEQUEST_READY);
            released_lumen = true;
        }
        mandelbrot16_probe_readback_ok = probe.readback_ok;
        mandelbrot16_probe_finished = probe.finished;
        crate::log!(
            "mandelbrot-gpu-sidequest: mandelbrot16-simd16-mul-dword-witness-probe submitted={} finished={} readback_ok={} reason={} program_source={} target_gpu=0x{:X} validation_hit_mask=0x{:04X} validation_scope=first-kickoff-lane0-only lane_dispatch_delta={} finish_marker=0x{:08X} lumen_released={} ready_gate=readback_ok artifact_contract=simd16-mul-dword-witness-split-src1-store-data-source-unfixed action={} next={} proves={} does_not_prove=full-frame-mandelbrot\n",
            probe.submitted as u8,
            probe.finished as u8,
            probe.readback_ok as u8,
            probe.reason,
            probe.program_name,
            probe.output_gpu,
            probe.output_hits_lo64 as u32,
            probe.dispatch_delta,
            probe.finish_marker,
            released_lumen as u8,
            if probe.submitted {
                "stage-simd16-mul-dword-witness-proof"
            } else {
                "hold-before-math-contract"
            },
            if probe.readback_ok {
                "restore-fixed10-q12-body-after-mul-witness"
            } else if probe.finished {
                "fix-simd16-mul-witness-readback-or-store"
            } else {
                "fix-simd16-mul-witness-submit-or-eot"
            },
            if probe.readback_ok {
                "boot-exercises-simd16-mul-dword-witness-store-eot-lane0-validation-once"
            } else if probe.dispatch_delta != 0 {
                "boot-exercises-new-mandelbrot16-artifact-partial-store"
            } else {
                "boot-attempts-new-mandelbrot16-artifact"
            },
        );
    }
    let run_legacy_row_writer = if mandelbrot16_probe_readback_ok {
        MANDELBROT_GPGPU_RUN_LEGACY_ROW_WRITER_FALLBACK_ON_SUCCESS
    } else {
        MANDELBROT_GPGPU_RUN_LEGACY_ROW_WRITER_FALLBACK_ON_FAILURE
    };
    if !run_legacy_row_writer {
        let stage_reason = if mandelbrot16_probe_readback_ok {
            "simd16-mul-dword-witness-proven"
        } else {
            "legacy-row-writer-disabled-without-mandelbrot16-proof"
        };
        let stage_next = if mandelbrot16_probe_readback_ok {
            "restore-fixed10-q12-body-after-mul-witness"
        } else if mandelbrot16_probe_finished {
            "fix-simd16-mul-witness-readback-or-store"
        } else {
            "fix-simd16-mul-witness-submit-or-eot"
        };
        crate::log!(
            "mandelbrot-gpu-sidequest: stage-parked reason={} legacy_row_writer_running=0 frame_loop_running=0 artifact_contract=simd16-mul-dword-witness-split-src1-store-data-source-unfixed next={} does_not_prove=full-frame-mandelbrot\n",
            stage_reason,
            stage_next,
        );
        loop {
            Timer::after(EmbassyDuration::from_millis(MANDELBROT_GPGPU_STAGE_READY_HEARTBEAT_MS))
                .await;
            frame = frame.wrapping_add(1);
            if frame == 1 || frame % 16 == 0 {
                crate::log!(
                    "mandelbrot-gpu-sidequest: stage-ready heartbeat={} reason={} artifact_stage=simd16-mul-dword-witness legacy_row_writer_running=0 next={} does_not_prove=full-frame-mandelbrot\n",
                    frame,
                    stage_reason,
                    stage_next,
                );
            }
        }
    }
    crate::log!(
        "mandelbrot-gpu-sidequest: legacy-row-writer-fallback-entered reason={} visual_only=1 next=return-to-mandelbrot16-after-store-or-submit-fix does_not_prove=mandelbrot-iteration-math\n",
        if mandelbrot16_probe_readback_ok {
            "enabled-after-success"
        } else {
            "mandelbrot16-probe-not-proven"
        },
    );
    let line_pixels = trueos_eu::gfx12::PRIMARY_SCANOUT_LINE1280_SCALAR_BW_LANES as u64;
    let rect_w = core::cmp::min(
        core::cmp::min(MANDELBROT_GPGPU_RGB_ZOOM_RECT_WIDTH, scanout_w as u64),
        line_pixels,
    );
    let rect_x = (scanout_w as u64).saturating_sub(rect_w) / 2;
    let rows_per_segment = 1u64;
    let segments_per_row =
        core::cmp::max(1, rect_w.saturating_add(line_pixels.saturating_sub(1)) / line_pixels);
    let mut burst_cursor = 0u64;
    let mut control_version_seen = u64::MAX;
    loop {
        let control = mandelbrot_artifact_control_snapshot();
        if control.version != control_version_seen {
            BURST_SLOT_COORDINATOR.store(0, Ordering::Release);
            burst_cursor = 0;
            control_version_seen = control.version;
            crate::log!(
                "mandelbrot-gpu-sidequest: control version={} flags=0x{:X} rect_h={} row_burst={} frame_budget={} phase_rows={} protocol=kernel-artifact-control\n",
                control.version,
                control.flags,
                control.rect_height_rows,
                control.row_groups_per_burst,
                control.bursts_per_frame_budget,
                control.phase_rows_per_sweep,
            );
        }
        let rect_h = core::cmp::min(control.rect_height_rows as u64, scanout_h as u64);
        let rect_y = (scanout_h as u64).saturating_sub(rect_h) / 2;
        let row_groups_per_frame = core::cmp::max(
            1,
            rect_h.saturating_add(rows_per_segment.saturating_sub(1)) / rows_per_segment,
        );
        let submits_per_tick = row_groups_per_frame;
        let row_groups_per_burst = core::cmp::max(
            1,
            core::cmp::min(row_groups_per_frame, control.row_groups_per_burst as u64),
        );
        let bursts_per_frame = row_groups_per_frame
            .saturating_add(row_groups_per_burst.saturating_sub(1))
            / row_groups_per_burst;
        let bursts_per_frame_budget = core::cmp::max(
            1,
            core::cmp::min(control.bursts_per_frame_budget as u64, bursts_per_frame),
        );
        let frames_per_sweep = core::cmp::max(
            1,
            bursts_per_frame.saturating_add(bursts_per_frame_budget.saturating_sub(1))
                / bursts_per_frame_budget,
        );
        let first_serial = frame.saturating_mul(submits_per_tick);
        let sweep_frame = frame / frames_per_sweep;
        let mut submitted = 0u64;
        let mut finished = 0u64;
        let mut readback_ok = 0u64;
        let mut dispatch_delta = 0u64;
        let mut burst = 0u64;
        let mut last_proof = None;
        let burst_window_start = burst_cursor;
        let phase_rows = sweep_frame.wrapping_mul(control.phase_rows_per_sweep as u64);
        while burst < bursts_per_frame_budget {
            let burst_slot = assign_burst_slot();
            let burst_index = (burst_window_start + burst) % bursts_per_frame;
            let local_row_group = burst_index.saturating_mul(row_groups_per_burst);
            let row_groups_this_burst = core::cmp::min(
                row_groups_per_burst,
                row_groups_per_frame.saturating_sub(local_row_group),
            );
            let local_row = local_row_group.saturating_mul(rows_per_segment);
            let band = local_row.saturating_add(phase_rows) / MANDELBROT_GPGPU_ANIM_BAND_HEIGHT;
            let color_seed = if control.full_frame_color_flip() {
                MANDELBROT_GPGPU_ANIM_PALETTE[(sweep_frame & 7) as usize]
            } else {
                MANDELBROT_GPGPU_ANIM_PALETTE[((band.saturating_add(sweep_frame)) & 7) as usize]
            };
            let proof =
                crate::intel::submit_gpgpu_primary_scanout_line1280_groupid_rows_fullwidth_color_burst(
                    color_seed,
                    local_row_group as u32,
                    row_groups_this_burst as u32,
                    rect_x as u32,
                    rect_y as u32,
                    rect_w as u32,
                    rect_h as u32,
                );
            let burst_snapshot = crate::chronos::latest_snapshot();
            if frame < 2 || frame % 64 == 0 {
                crate::log!(
                    "mandelbrot-gpu-sidequest: burst frame={} idx={} slot={} rows={}..+{} seed=0x{:08X} result={} ms={} ticks={} seq={}\n",
                    frame,
                    burst,
                    burst_slot,
                    local_row_group,
                    row_groups_this_burst,
                    color_seed,
                    if proof.submitted { "ok" } else { "fail" },
                    burst_snapshot.mono_ms,
                    burst_snapshot.mono_ticks,
                    burst_snapshot.seq
                );
            }
            if proof.submitted {
                submitted = submitted.saturating_add(row_groups_this_burst);
            }
            if proof.finished {
                finished = finished.saturating_add(row_groups_this_burst);
            }
            if proof.readback_ok {
                readback_ok = readback_ok.saturating_add(row_groups_this_burst);
            }
            dispatch_delta = dispatch_delta.saturating_add(proof.dispatch_delta as u64);
            if proof.submitted && !released_lumen {
                crate::r::readiness::set(crate::r::readiness::MANDELBROT_GPU_SIDEQUEST_READY);
                released_lumen = true;
            }
            let proof_ok =
                proof.readback_ok || (control.visual_only_ignore_retire() && proof.submitted);
            last_proof = Some(proof);
            if !proof_ok {
                break;
            }
            burst += 1;
        }
        if bursts_per_frame != 0 {
            burst_cursor = (burst_window_start + burst) % bursts_per_frame;
        }
        let frame_notified = submitted != 0
            && (readback_ok == submitted || control.visual_only_ignore_retire())
            && control.notify_after_sweep()
            && crate::intel::notify_gpgpu_primary_scanout_external_write(
                "gpgpu-primary-scanout-line1280-frame",
                0,
                MANDELBROT_GPGPU_PRESENT_FLUSH_BYTES,
            );

        let telemetry_failed = readback_ok != submitted && !control.visual_only_ignore_retire();
        let should_log_frame = frame < 4 || frame % 64 == 0 || telemetry_failed;
        if should_log_frame && let Some(last_proof) = last_proof {
            crate::log!(
                "mandelbrot-gpu-sidequest: frame-summary frame={} sweep_frame={} frames_per_sweep={} burst_cursor={} bursts_actual={} bursts_budget={} bursts_max={} slot_pool={}..+{} control_version={} gpu=0x{:X} fps_nominal=60 visual_only={}\n",
                frame,
                sweep_frame,
                frames_per_sweep,
                burst_window_start,
                burst,
                bursts_per_frame_budget,
                bursts_per_frame,
                MANDELBROT_BURST_LANE_POOL_START,
                MANDELBROT_BURST_LANE_POOL_SIZE,
                control.version,
                last_proof.output_gpu,
                control.visual_only_ignore_retire() as u8
            );
        }
        if should_log_frame && let Some(last_proof) = last_proof {
            crate::log!(
                "mandelbrot-gpu-sidequest: gpgpu-primary-framebuffer-visible-line1280-groupid-row-loop frame={} first_serial={} rect={}x{}@{},{} segments_per_row={} rows_per_segment={} row_groups_per_frame={} row_groups_per_burst={} bursts_per_frame={} walker_submits_per_frame={} full_frame_color_flip={} band_height={} phase_rows={} submitted={} finished={} readback_ok={} frame_notified={} reason={} program_source={} target_gpu=0x{:X} sample_before=0x{:08X} sample_after=0x{:08X} sample_change_mask=0x{:016X} lane_dispatch_delta={} finish_marker=0x{:08X} lumen_released={} action={} next={} deliverable=visible-window-line1280-groupid-row-animated\n",
                frame,
                first_serial,
                rect_w,
                rect_h,
                rect_x,
                rect_y,
                segments_per_row,
                rows_per_segment,
                row_groups_per_frame,
                row_groups_per_burst,
                bursts_per_frame,
                bursts_per_frame_budget,
                control.full_frame_color_flip() as u8,
                MANDELBROT_GPGPU_ANIM_BAND_HEIGHT,
                phase_rows,
                submitted,
                finished,
                readback_ok,
                frame_notified as u8,
                last_proof.reason,
                last_proof.program_name,
                last_proof.output_gpu,
                last_proof.output_first_before,
                last_proof.output_first_after,
                last_proof.output_hits_lo64,
                dispatch_delta,
                last_proof.finish_marker,
                released_lumen as u8,
                if submitted != 0 {
                    "continue-fullscreen-line-pilot"
                } else {
                    "hold-fullscreen-line-pilot"
                },
                if readback_ok == submitted {
                    "continue-fullscreen-fill"
                } else if control.visual_only_ignore_retire() && submitted != 0 {
                    "continue-visual-groupid-flow"
                } else if dispatch_delta != 0 {
                    "fix-fullscreen-line-store"
                } else {
                    "fix-fullscreen-line-submit"
                },
            );
        }
        frame = frame.wrapping_add(1);
        Timer::after(EmbassyDuration::from_millis(MANDELBROT_GPGPU_LOOP_MS)).await;
    }
}
