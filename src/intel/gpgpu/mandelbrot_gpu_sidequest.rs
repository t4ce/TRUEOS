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
pub(crate) const MANDELBROT16_BOOT_Q12_MODE_ONE_ITER_VISIBLE: u32 = 43;
pub(crate) const MANDELBROT16_BOOT_Q12_MODE_FIXED10_VISIBLE: u32 = 44;
pub(crate) const MANDELBROT16_BOOT_Q12_MODE_FIXED1_FEEDBACK_VISIBLE: u32 = 45;
pub(crate) const MANDELBROT16_BOOT_Q12_C_RE: u32 = 0x0000_0800;
pub(crate) const MANDELBROT16_BOOT_Q12_C_IM: u32 = 0x0000_0400;
pub(crate) const MANDELBROT16_BOOT_Q12_EXPECTED_RE1: u32 = 0x0000_0B00;
pub(crate) const MANDELBROT16_BOOT_Q12_EXPECTED_VISIBLE: u32 = 0xFFFF_0B00;
pub(crate) const MANDELBROT16_Q12_VIEW_RE_MIN: i32 = -8192;
pub(crate) const MANDELBROT16_Q12_VIEW_RE_SPAN: i32 = 12_288;
pub(crate) const MANDELBROT16_Q12_VIEW_IM_MIN: i32 = -4608;
pub(crate) const MANDELBROT16_Q12_VIEW_IM_SPAN: i32 = 9216;
pub(crate) const MANDELBROT16_T13_Q12_X_STEP: i32 = 5;
pub(crate) const MANDELBROT16_BOOT_VISIBLE_STAMP_ROWS: u32 = 32;
pub(crate) const MANDELBROT16_STAGE_READY_SWEEP_ENABLED: bool = true;
pub(crate) const MANDELBROT16_STAGE_READY_SWEEP_X_BLOCKS_PER_HEARTBEAT: u32 = 160;
pub(crate) const MANDELBROT16_STAGE_READY_ROW_GROUP_SCALE: [u32; 1] = [1];
pub(crate) const MANDELBROT16_STAGE_READY_X_BLOCK_SCALE: [u32; 1] = [160];
pub(crate) const MANDELBROT16_T11_LINEAR_MAX_GROUPS_PER_SUBMIT: u32 = 32;
pub(crate) const MANDELBROT16_T30_FULLSCREEN_ENABLED: bool = true;
pub(crate) const MANDELBROT16_T30_ROWS_PER_SUBMIT: u32 = 20;
pub(crate) const MANDELBROT16_T30_BANDS_PER_HEARTBEAT: u32 = 1;
pub(crate) const MANDELBROT16_T30_IMMEDIATE_X_BLOCKS_PER_HEARTBEAT: u32 = 32;
pub(crate) const MANDELBROT16_T30_IMMEDIATE_LANE0_SWEEP_ENABLED: bool = true;
pub(crate) const MANDELBROT16_T30_IMMEDIATE_LANE_PHASES_PER_BLOCK: u32 = 1;
pub(crate) const MANDELBROT16_T30_ADVANCE_ROW_CURSOR: bool = true;
pub(crate) const MANDELBROT16_T30_GROUPID_DIAGNOSTIC_ENABLED: bool = false;
pub(crate) const MANDELBROT16_T30_BASELINE_HALFSCREEN_ENABLED: bool = false;
pub(crate) const MANDELBROT16_T30_BASELINE_HALFSCREEN_ROWS_PER_HEARTBEAT: u32 = 16;
pub(crate) const MANDELBROT16_T30_BASELINE_HALFSCREEN_COLOR: u32 = 0xFFFF_0000;
pub(crate) const MANDELBROT16_T31_STORE_LADDER_ENABLED: bool = false;
pub(crate) const MANDELBROT16_T31_STORE_LADDER_PERIOD_HEARTBEATS: u64 = 64;
pub(crate) const MANDELBROT16_T32_SINGLE_SEND_PROBE_ENABLED: bool = false;
pub(crate) const MANDELBROT16_T33_BTI1_UNTYPED_PROBE_ENABLED: bool = false;
pub(crate) const MANDELBROT16_T34_ADDRESS_DATA_WITNESS_ENABLED: bool = false;
pub(crate) const MANDELBROT16_T35_EXPLICIT_WIDE_PAYLOAD_ENABLED: bool = false;
pub(crate) const MANDELBROT16_T36_UNROLLED_SCALAR16_ENABLED: bool = false;
pub(crate) const MANDELBROT16_T37_GROUPID_X_WITNESS_ENABLED: bool = false;
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

fn mandelbrot16_block_c_re_q12(x_group: u32, x_groups: u32) -> u32 {
    let denom = x_groups.saturating_sub(1).max(1) as i32;
    let value = MANDELBROT16_Q12_VIEW_RE_MIN
        + (MANDELBROT16_Q12_VIEW_RE_SPAN.saturating_mul(x_group as i32) / denom);
    value as i16 as u16 as u32
}

fn mandelbrot16_pixel_c_re_q12(pixel_x: u32) -> u32 {
    let value = MANDELBROT16_Q12_VIEW_RE_MIN
        .saturating_add(MANDELBROT16_T13_Q12_X_STEP.saturating_mul(pixel_x as i32));
    value as i16 as u16 as u32
}

fn mandelbrot16_block_c_im_q12(y_block: u32, y_blocks: u32) -> u32 {
    let denom = y_blocks.saturating_sub(1).max(1) as i32;
    let value = MANDELBROT16_Q12_VIEW_IM_MIN
        + (MANDELBROT16_Q12_VIEW_IM_SPAN.saturating_mul(y_block as i32) / denom);
    value as i16 as u16 as u32
}

fn mandelbrot16_visible_constant_color(frame: u64, x_group: u32, y_block: u32) -> u32 {
    const COLORS: [u32; 8] = [
        0xFFFF_00FF,
        0xFF00_FFFF,
        0xFFFF_FF00,
        0xFFFF_3300,
        0xFF33_FF00,
        0xFF00_33FF,
        0xFFFF_FFFF,
        0xFF88_00FF,
    ];
    let idx = ((frame as u32 / 4)
        .wrapping_add(x_group)
        .wrapping_add(y_block.saturating_mul(3))
        & 7) as usize;
    COLORS[idx]
}

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
        "mandelbrot-gpu-sidequest: attempted name={} called=1 hot=1 artifact_stage=gpgpu-eu-q12-simd16-fixed10-escape-gradient target={}x{} scanout={}x{} primary_gpu=0x{:X} simd16_store_probe_enabled={} q12_simd8x4_reference_enabled={} legacy_row_fallback_on_success={} legacy_row_fallback_on_failure={} action=boot-exercise-simd16-q12-one-iteration-visible-store-then-fixed10-escape-gradient-sweep cpu_runtime_patches=address-plus-linear-chunk-q12-coordinate eu_runtime_work=address-derived-lane-q12-re-plus-row-chunk-q12-ci-fixed10-escape-accumulation-gradient-store-eot q12_c_re=0x{:08X} q12_c_im=0x{:08X} expected_q12_re1=0x{:08X} expected_plane_value=0x{:08X} next=increase-iteration-budget-or-refine-gradient does_not_prove=single-heartbeat-full-frame-or-smooth-coloring\n",
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
        MANDELBROT16_BOOT_Q12_C_RE,
        MANDELBROT16_BOOT_Q12_C_IM,
        MANDELBROT16_BOOT_Q12_EXPECTED_RE1,
        MANDELBROT16_BOOT_Q12_EXPECTED_VISIBLE,
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
    let mut mandelbrot16_t17_immediate_constant_readback_ok = false;
    let mut mandelbrot16_t17_immediate_constant_finished = false;
    let mut mandelbrot16_t17_immediate_constant_sample_match = false;
    let mut mandelbrot16_t20_immediate_row_constant_readback_ok = false;
    let mut mandelbrot16_t20_immediate_row_constant_finished = false;
    let mut mandelbrot16_t16_linear_constant_readback_ok = false;
    let mut mandelbrot16_t16_linear_constant_finished = false;
    let mut mandelbrot16_t15_probe_readback_ok = false;
    let mut mandelbrot16_t15_probe_finished = false;
    let mut mandelbrot16_t19_raw_radius_readback_ok = false;
    let mut mandelbrot16_t19_raw_radius_finished = false;
    if MANDELBROT_GPGPU_RUN_MANDELBROT16_SIMD16_STORE_PROBE {
        let x_base = ((scanout_w as u32).saturating_sub(
            trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_PIXELS_PER_PROGRAM as u32,
        )) / 2;
        let row_index = ((scanout_h as u32) / 2).saturating_sub(16);
        let mut probe =
            crate::intel::submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_probe(
                MANDELBROT16_BOOT_Q12_MODE_ONE_ITER_VISIBLE,
                row_index,
                x_base,
                MANDELBROT16_BOOT_Q12_C_RE,
                MANDELBROT16_BOOT_Q12_C_IM,
            );
        let mut stamped_rows: u32 = if probe.readback_ok { 1 } else { 0 };
        let mut stamp_row = 1u32;
        while stamp_row < MANDELBROT16_BOOT_VISIBLE_STAMP_ROWS {
            let row_probe =
                crate::intel::submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_probe(
                    MANDELBROT16_BOOT_Q12_MODE_ONE_ITER_VISIBLE,
                    row_index.saturating_add(stamp_row),
                    x_base,
                    MANDELBROT16_BOOT_Q12_C_RE,
                    MANDELBROT16_BOOT_Q12_C_IM,
                );
            if row_probe.readback_ok {
                stamped_rows = stamped_rows.saturating_add(1);
            }
            if !probe.readback_ok && (row_probe.submitted || row_probe.finished) {
                probe = row_probe;
            }
            stamp_row = stamp_row.saturating_add(1);
        }
        if probe.readback_ok && !released_lumen {
            crate::r::readiness::set(crate::r::readiness::MANDELBROT_GPU_SIDEQUEST_READY);
            released_lumen = true;
        }
        mandelbrot16_probe_readback_ok = probe.readback_ok;
        mandelbrot16_probe_finished = probe.finished;
        crate::log!(
            "mandelbrot-gpu-sidequest: mandelbrot16-simd16-q12-one-iter-visible-plane-probe submitted={} finished={} readback_ok={} reason={} program_source={} target_gpu=0x{:X} stamp_rect={}x{}@{},{} stamped_rows={} validation_hit_mask=0x{:04X} validation_scope=first-kickoff-lane0-only lane_dispatch_delta={} finish_marker=0x{:08X} lumen_released={} ready_gate=readback_ok artifact_contract=simd16-q12-one-iteration-visible-store-primary-plane q12_c_re=0x{:08X} q12_c_im=0x{:08X} expected_q12_re1=0x{:08X} expected_plane_value=0x{:08X} action={} next={} proves={} does_not_prove=full-frame-mandelbrot\n",
            probe.submitted as u8,
            probe.finished as u8,
            probe.readback_ok as u8,
            probe.reason,
            probe.program_name,
            probe.output_gpu,
            trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_PIXELS_PER_PROGRAM,
            MANDELBROT16_BOOT_VISIBLE_STAMP_ROWS,
            x_base,
            row_index,
            stamped_rows,
            probe.output_hits_lo64 as u32,
            probe.dispatch_delta,
            probe.finish_marker,
            released_lumen as u8,
            MANDELBROT16_BOOT_Q12_C_RE,
            MANDELBROT16_BOOT_Q12_C_IM,
            MANDELBROT16_BOOT_Q12_EXPECTED_RE1,
            MANDELBROT16_BOOT_Q12_EXPECTED_VISIBLE,
            if probe.submitted {
                "stage-simd16-q12-one-iteration-visible-plane-proof"
            } else {
                "hold-before-math-contract"
            },
            if probe.readback_ok {
                "start-fixed10-escape-bw-sweep"
            } else if probe.finished {
                "fix-simd16-q12-address-or-store-readback"
            } else {
                "fix-simd16-q12-submit-or-eot"
            },
            if probe.readback_ok {
                "boot-exercises-simd16-q12-one-iteration-visible-store-eot-primary-plane-lane0-validation-once"
            } else if probe.dispatch_delta != 0 {
                "boot-exercises-new-mandelbrot16-artifact-partial-store"
            } else {
                "boot-attempts-new-mandelbrot16-artifact"
            },
        );
        if mandelbrot16_probe_readback_ok {
            const T17_IMMEDIATE_CONSTANT_COLOR: u32 = 0xFF33_CC00;
            const T16_LINEAR_CONSTANT_COLOR: u32 = 0xFF33_CC00;
            let t17_probe =
                crate::intel::submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_immediate_constant_probe(
                    row_index,
                    x_base,
                    T17_IMMEDIATE_CONSTANT_COLOR,
                );
            mandelbrot16_t17_immediate_constant_readback_ok = t17_probe.readback_ok;
            mandelbrot16_t17_immediate_constant_finished = t17_probe.finished;
            mandelbrot16_t17_immediate_constant_sample_match =
                t17_probe.finished && t17_probe.output_first_after == t17_probe.sentinel;
            crate::log!(
                "mandelbrot-gpu-sidequest: t17-immediate-constant-store-probe submitted={} finished={} readback_ok={} reason={} program_source={} target_gpu=0x{:X} stamp_rect={}x{}@{},{} validation_hit_mask=0x{:04X} lane_dispatch_delta={} finish_marker=0x{:08X} artifact_contract=simd16-t17-immediate-base-constant-primary-plane sample_before=0x{:08X} sample_after=0x{:08X} sample_expected=0x{:08X} sample_match={} action={} next={} proves={} does_not_prove=linear-address-or-t15-gradient-math-or-full-frame-mandelbrot\n",
                t17_probe.submitted as u8,
                t17_probe.finished as u8,
                t17_probe.readback_ok as u8,
                t17_probe.reason,
                t17_probe.program_name,
                t17_probe.output_gpu,
                trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_PIXELS_PER_PROGRAM,
                1u32,
                x_base,
                row_index,
                t17_probe.output_hits_lo64 as u32,
                t17_probe.dispatch_delta,
                t17_probe.finish_marker,
                t17_probe.output_first_before,
                t17_probe.output_first_after,
                t17_probe.sentinel,
                mandelbrot16_t17_immediate_constant_sample_match as u8,
                if mandelbrot16_t17_immediate_constant_sample_match {
                    "run-t16-linear-address-probe"
                } else {
                    "park-before-linear-address-probe"
                },
                if t17_probe.readback_ok {
                    "check-linear-address-prelude"
                } else if mandelbrot16_t17_immediate_constant_sample_match {
                    "check-linear-address-prelude-after-lane0-real-write"
                } else if t17_probe.finished {
                    "fix-constant-store-body-or-send-payload"
                } else {
                    "fix-t17-immediate-constant-submit-or-eot"
                },
                if t17_probe.readback_ok {
                    "t17-immediate-base-plus-constant-store-eot-readback-validation"
                } else if t17_probe.dispatch_delta != 0 {
                    "t17-immediate-base-dispatch-eot-but-constant-store-mismatch"
                } else {
                    "t17-immediate-base-submit-attempted"
                },
            );
            if mandelbrot16_t17_immediate_constant_sample_match {
                let t20_row_probe =
                    crate::intel::submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_immediate_constant_rows_probe(
                        row_index,
                        x_base,
                        MANDELBROT16_BOOT_VISIBLE_STAMP_ROWS,
                        T17_IMMEDIATE_CONSTANT_COLOR,
                    );
                mandelbrot16_t20_immediate_row_constant_readback_ok = t20_row_probe.readback_ok;
                mandelbrot16_t20_immediate_row_constant_finished = t20_row_probe.finished;
                crate::log!(
                    "mandelbrot-gpu-sidequest: t20-immediate-row-constant-store-probe submitted={} finished={} readback_ok={} reason={} program_source={} target_gpu=0x{:X} stamp_rect={}x{}@{},{} validation_hit_mask=0x{:04X} lane_dispatch_delta={} finish_marker=0x{:08X} artifact_contract=simd16-t20-immediate-base-row32-constant-primary-plane sample_before=0x{:08X} sample_after=0x{:08X} sample_expected=0x{:08X} sample_match={} action={} next={} proves={} does_not_prove=t19-raw-radius-math-or-full-frame-mandelbrot\n",
                    t20_row_probe.submitted as u8,
                    t20_row_probe.finished as u8,
                    t20_row_probe.readback_ok as u8,
                    t20_row_probe.reason,
                    t20_row_probe.program_name,
                    t20_row_probe.output_gpu,
                    trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_PIXELS_PER_PROGRAM,
                    MANDELBROT16_BOOT_VISIBLE_STAMP_ROWS,
                    x_base,
                    row_index,
                    t20_row_probe.output_hits_lo64 as u32,
                    t20_row_probe.dispatch_delta,
                    t20_row_probe.finish_marker,
                    t20_row_probe.output_first_before,
                    t20_row_probe.output_first_after,
                    t20_row_probe.sentinel,
                    (t20_row_probe.output_first_after == t20_row_probe.sentinel) as u8,
                    if t20_row_probe.readback_ok {
                        "row-group-address-path-ok"
                    } else {
                        "hold-row-group-sweep"
                    },
                    if t20_row_probe.readback_ok {
                        "fix-gradient-compare-accumulator-or-raw-radius-payload"
                    } else if t20_row_probe.finished {
                        "fix-immediate-row-address-prelude"
                    } else {
                        "fix-t20-submit-or-eot"
                    },
                    if t20_row_probe.readback_ok {
                        "t20-immediate-row32-constant-store-eot-readback-validation"
                    } else if t20_row_probe.dispatch_delta != 0 {
                        "t20-row32-dispatch-eot-but-store-mismatch"
                    } else {
                        "t20-row32-submit-attempted"
                    },
                );
                let t16_probe =
                    crate::intel::submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_linear_constant_probe(
                        row_index,
                        x_base,
                        T16_LINEAR_CONSTANT_COLOR,
                    );
                mandelbrot16_t16_linear_constant_readback_ok = t16_probe.readback_ok;
                mandelbrot16_t16_linear_constant_finished = t16_probe.finished;
                crate::log!(
                    "mandelbrot-gpu-sidequest: t16-linear-constant-store-probe submitted={} finished={} readback_ok={} reason={} program_source={} target_gpu=0x{:X} stamp_rect={}x{}@{},{} validation_hit_mask=0x{:04X} lane_dispatch_delta={} finish_marker=0x{:08X} artifact_contract=simd16-t16-linear-groupid-constant-primary-plane sample_before=0x{:08X} sample_after=0x{:08X} sample_expected=0x{:08X} sample_match={} action={} next={} proves={} does_not_prove=t15-gradient-math-or-full-frame-mandelbrot\n",
                    t16_probe.submitted as u8,
                    t16_probe.finished as u8,
                    t16_probe.readback_ok as u8,
                    t16_probe.reason,
                    t16_probe.program_name,
                    t16_probe.output_gpu,
                    trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_PIXELS_PER_PROGRAM,
                    1u32,
                    x_base,
                    row_index,
                    t16_probe.output_hits_lo64 as u32,
                    t16_probe.dispatch_delta,
                    t16_probe.finish_marker,
                    t16_probe.output_first_before,
                    t16_probe.output_first_after,
                    t16_probe.sentinel,
                    (t16_probe.readback_ok && t16_probe.output_first_after == t16_probe.sentinel)
                        as u8,
                    if t16_probe.readback_ok {
                        "run-t15-gradient-probe"
                    } else if t16_probe.finished {
                        "run-t15-immediate-gradient-probe-keep-t16-diagnostic"
                    } else {
                        "park-before-t15-gradient-sweep"
                    },
                    if t16_probe.readback_ok {
                        "check-t15-gradient-math-payload"
                    } else if t16_probe.finished {
                        "validate-t15-gradient-on-proven-immediate-address"
                    } else {
                        "fix-linear-constant-submit-or-eot"
                    },
                    if t16_probe.readback_ok {
                        "t16-linear-address-plus-constant-store-eot-readback-validation"
                    } else if t16_probe.dispatch_delta != 0 {
                        "t16-linear-address-dispatch-eot-but-store-mismatch"
                    } else {
                        "t16-linear-address-submit-attempted"
                    },
                );
                if t16_probe.finished {
                    let t15_probe_c_re = mandelbrot16_pixel_c_re_q12(x_base);
                    let t15_probe_c_im = mandelbrot16_block_c_im_q12(
                        row_index / MANDELBROT16_BOOT_VISIBLE_STAMP_ROWS,
                        (scanout_h as u32).saturating_add(MANDELBROT16_BOOT_VISIBLE_STAMP_ROWS - 1)
                            / MANDELBROT16_BOOT_VISIBLE_STAMP_ROWS,
                    );
                    let t15_probe = if t16_probe.readback_ok {
                        crate::intel::submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_linear_band_probe(
                            row_index,
                            x_base,
                            1,
                            1,
                            t15_probe_c_re,
                            t15_probe_c_im,
                        )
                    } else {
                        crate::intel::submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_immediate_gradient_probe(
                            row_index,
                            x_base,
                            t15_probe_c_re,
                            t15_probe_c_im,
                        )
                    };
                    mandelbrot16_t15_probe_readback_ok = t15_probe.readback_ok;
                    mandelbrot16_t15_probe_finished = t15_probe.finished;
                    crate::log!(
                        "mandelbrot-gpu-sidequest: t15-gradient-store-probe submitted={} finished={} readback_ok={} reason={} program_source={} target_gpu=0x{:X} stamp_rect={}x{}@{},{} address_path={} validation_hit_mask=0x{:04X} lane_dispatch_delta={} finish_marker=0x{:08X} artifact_contract={} sample_before=0x{:08X} sample_after=0x{:08X} sample_expected=0x{:08X} sample_match={} action={} next={} proves={} does_not_prove=physical-display-visible-or-single-heartbeat-full-frame-or-smooth-coloring\n",
                        t15_probe.submitted as u8,
                        t15_probe.finished as u8,
                        t15_probe.readback_ok as u8,
                        t15_probe.reason,
                        t15_probe.program_name,
                        t15_probe.output_gpu,
                        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_PIXELS_PER_PROGRAM,
                        1u32,
                        x_base,
                        row_index,
                        if t16_probe.readback_ok {
                            "linear-groupid"
                        } else {
                            "immediate-base-fallback"
                        },
                        t15_probe.output_hits_lo64 as u32,
                        t15_probe.dispatch_delta,
                        t15_probe.finish_marker,
                        if t16_probe.readback_ok {
                            "simd16-t15-linear-eu-lane-re-row-chunk-ci-fixed10-escape-gradient-primary-plane"
                        } else {
                            "simd16-t18-immediate-eu-lane-re-row-chunk-ci-fixed10-escape-gradient-primary-plane"
                        },
                        t15_probe.output_first_before,
                        t15_probe.output_first_after,
                        t15_probe.sentinel,
                        (t15_probe.readback_ok
                            && t15_probe.output_first_after == t15_probe.sentinel)
                            as u8,
                        if t15_probe.readback_ok {
                            "allow-gradient-sweep"
                        } else {
                            "park-t15-gradient-sweep"
                        },
                        if t15_probe.readback_ok {
                            "increase-iteration-budget-or-refine-gradient"
                        } else if t15_probe.finished {
                            "fix-t15-math-payload-or-final-store"
                        } else {
                            "fix-t15-submit-or-eot"
                        },
                        if t15_probe.readback_ok {
                            if t16_probe.readback_ok {
                                "t15-linear-single-group-gradient-store-eot-readback-validation"
                            } else {
                                "t18-immediate-single-group-gradient-store-eot-readback-validation"
                            }
                        } else if t15_probe.dispatch_delta != 0 {
                            "t15-single-group-dispatch-eot-but-store-mismatch"
                        } else {
                            "t15-single-group-submit-attempted"
                        },
                    );
                    if !t16_probe.readback_ok {
                        let t19_probe = crate::intel::submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_immediate_raw_radius_probe(
                            row_index,
                            x_base,
                            t15_probe_c_re,
                            t15_probe_c_im,
                        );
                        mandelbrot16_t19_raw_radius_readback_ok = t19_probe.readback_ok;
                        mandelbrot16_t19_raw_radius_finished = t19_probe.finished;
                        crate::log!(
                            "mandelbrot-gpu-sidequest: t19-raw-radius-store-probe submitted={} finished={} readback_ok={} reason={} program_source={} target_gpu=0x{:X} stamp_rect={}x{}@{},{} address_path=immediate-base-fallback validation_hit_mask=0x{:04X} lane_dispatch_delta={} finish_marker=0x{:08X} artifact_contract=simd16-t19-immediate-eu-lane-re-row-chunk-ci-fixed1-raw-radius-primary-plane sample_before=0x{:08X} sample_after=0x{:08X} sample_expected=0x{:08X} sample_match={} action={} next={} proves={} does_not_prove=gradient-compare-accumulator-or-physical-display-visible-or-single-heartbeat-full-frame\n",
                            t19_probe.submitted as u8,
                            t19_probe.finished as u8,
                            t19_probe.readback_ok as u8,
                            t19_probe.reason,
                            t19_probe.program_name,
                            t19_probe.output_gpu,
                            trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_PIXELS_PER_PROGRAM,
                            1u32,
                            x_base,
                            row_index,
                            t19_probe.output_hits_lo64 as u32,
                            t19_probe.dispatch_delta,
                            t19_probe.finish_marker,
                            t19_probe.output_first_before,
                            t19_probe.output_first_after,
                            t19_probe.sentinel,
                            (t19_probe.readback_ok
                                && t19_probe.output_first_after == t19_probe.sentinel)
                                as u8,
                            if t19_probe.readback_ok {
                                "allow-raw-radius-sweep"
                            } else {
                                "keep-gradient-sweep-if-available"
                            },
                            if t19_probe.readback_ok {
                                "fix-gradient-compare-accumulator-while-raw-radius-sweeps"
                            } else if t19_probe.finished {
                                "fix-raw-radius-final-store"
                            } else {
                                "fix-t19-submit-or-eot"
                            },
                            if t19_probe.readback_ok {
                                "t19-immediate-single-group-raw-radius-store-eot-readback-validation"
                            } else if t19_probe.dispatch_delta != 0 {
                                "t19-single-group-dispatch-eot-but-store-mismatch"
                            } else {
                                "t19-single-group-submit-attempted"
                            },
                        );
                    }
                }
            }
        }
    }
    let run_legacy_row_writer = if mandelbrot16_probe_readback_ok {
        MANDELBROT_GPGPU_RUN_LEGACY_ROW_WRITER_FALLBACK_ON_SUCCESS
    } else {
        MANDELBROT_GPGPU_RUN_LEGACY_ROW_WRITER_FALLBACK_ON_FAILURE
    };
    if !run_legacy_row_writer {
        let immediate_row_group_ready = mandelbrot16_t20_immediate_row_constant_readback_ok;
        let immediate_row_group_finished = mandelbrot16_t20_immediate_row_constant_finished;
        let t15_sweep_gate = mandelbrot16_probe_readback_ok
            && mandelbrot16_t17_immediate_constant_readback_ok
            && ((mandelbrot16_t16_linear_constant_readback_ok
                && mandelbrot16_t15_probe_readback_ok)
                || mandelbrot16_t15_probe_readback_ok
                || (immediate_row_group_ready
                    && (mandelbrot16_t15_probe_readback_ok
                        || mandelbrot16_t19_raw_radius_readback_ok)));
        let t15_linear_address_ready = mandelbrot16_t16_linear_constant_readback_ok;
        let t19_raw_radius_ready = !t15_linear_address_ready
            && immediate_row_group_ready
            && mandelbrot16_t19_raw_radius_readback_ok;
        let stage_reason = if t15_sweep_gate {
            if t19_raw_radius_ready {
                "simd16-q12-one-iteration-visible-primary-plane-proven-fixed1-raw-radius-immediate-sweep-enabled-gradient-diagnostic-failed-linear-diagnostic-failed"
            } else if t15_linear_address_ready {
                "simd16-q12-one-iteration-visible-primary-plane-proven-fixed10-escape-gradient-linear-sweep-enabled"
            } else if !immediate_row_group_ready {
                "simd16-q12-one-iteration-visible-primary-plane-proven-t17-constant-immediate-single-row-sweep-enabled-groupid-diagnostic-failed"
            } else {
                "simd16-q12-one-iteration-visible-primary-plane-proven-fixed10-escape-gradient-immediate-sweep-enabled-linear-diagnostic-failed"
            }
        } else if mandelbrot16_probe_readback_ok
            && mandelbrot16_t17_immediate_constant_finished
            && mandelbrot16_t17_immediate_constant_sample_match
            && !mandelbrot16_t17_immediate_constant_readback_ok
        {
            "t10-visible-store-proven-and-t17-lane0-real-but-t17-all-lanes-mismatch"
        } else if mandelbrot16_probe_readback_ok
            && mandelbrot16_t17_immediate_constant_finished
            && !mandelbrot16_t17_immediate_constant_readback_ok
        {
            "t10-visible-store-proven-but-t17-immediate-constant-store-mismatch"
        } else if mandelbrot16_probe_readback_ok && !mandelbrot16_t17_immediate_constant_finished {
            "t10-visible-store-proven-but-t17-immediate-constant-submit-not-finished"
        } else if mandelbrot16_probe_readback_ok
            && mandelbrot16_t17_immediate_constant_readback_ok
            && immediate_row_group_finished
            && !immediate_row_group_ready
        {
            "t10-and-t17-store-proven-but-t20-immediate-row-store-mismatch"
        } else if mandelbrot16_probe_readback_ok
            && mandelbrot16_t17_immediate_constant_readback_ok
            && !immediate_row_group_finished
        {
            "t10-and-t17-store-proven-but-t20-immediate-row-submit-not-finished"
        } else if mandelbrot16_probe_readback_ok
            && mandelbrot16_t17_immediate_constant_readback_ok
            && mandelbrot16_t16_linear_constant_finished
            && !mandelbrot16_t16_linear_constant_readback_ok
        {
            "t10-and-t17-store-proven-but-t16-linear-address-store-mismatch"
        } else if mandelbrot16_probe_readback_ok
            && mandelbrot16_t17_immediate_constant_readback_ok
            && !mandelbrot16_t16_linear_constant_finished
        {
            "t10-and-t17-store-proven-but-t16-linear-address-submit-not-finished"
        } else if mandelbrot16_probe_readback_ok
            && (mandelbrot16_t15_probe_finished || mandelbrot16_t19_raw_radius_finished)
        {
            "t10-visible-store-proven-but-t15-gradient-store-mismatch"
        } else if mandelbrot16_probe_readback_ok {
            "t10-visible-store-proven-but-t15-gradient-submit-not-finished"
        } else {
            "legacy-row-writer-disabled-without-mandelbrot16-proof"
        };
        let stage_next = if t15_sweep_gate {
            if t19_raw_radius_ready {
                "fix-gradient-compare-accumulator-while-raw-radius-sweeps"
            } else if t15_linear_address_ready {
                "increase-iteration-budget-or-refine-gradient"
            } else if !immediate_row_group_ready {
                "paint-fast-t17-constant-single-row-sweep-while-fixing-groupid-address-prelude"
            } else {
                "fix-linear-address-prelude-while-immediate-gradient-sweeps"
            }
        } else if mandelbrot16_probe_readback_ok
            && mandelbrot16_t17_immediate_constant_finished
            && mandelbrot16_t17_immediate_constant_sample_match
            && !mandelbrot16_t17_immediate_constant_readback_ok
        {
            "run-t30-from-real-lane0-while-fixing-simd16-payload"
        } else if mandelbrot16_probe_readback_ok
            && mandelbrot16_t17_immediate_constant_finished
            && !mandelbrot16_t17_immediate_constant_readback_ok
        {
            "fix-constant-store-body-or-send-payload"
        } else if mandelbrot16_probe_readback_ok && !mandelbrot16_t17_immediate_constant_finished {
            "fix-t17-immediate-constant-submit-or-eot"
        } else if mandelbrot16_probe_readback_ok
            && mandelbrot16_t17_immediate_constant_readback_ok
            && immediate_row_group_finished
            && !immediate_row_group_ready
        {
            "fix-immediate-row-address-prelude"
        } else if mandelbrot16_probe_readback_ok
            && mandelbrot16_t17_immediate_constant_readback_ok
            && !immediate_row_group_finished
        {
            "fix-t20-immediate-row-submit-or-eot"
        } else if mandelbrot16_probe_readback_ok
            && mandelbrot16_t17_immediate_constant_readback_ok
            && mandelbrot16_t16_linear_constant_finished
            && !mandelbrot16_t16_linear_constant_readback_ok
        {
            "fix-linear-address-prelude"
        } else if mandelbrot16_probe_readback_ok
            && mandelbrot16_t17_immediate_constant_readback_ok
            && !mandelbrot16_t16_linear_constant_finished
        {
            "fix-t16-linear-constant-submit-or-eot"
        } else if mandelbrot16_probe_readback_ok
            && (mandelbrot16_t15_probe_finished || mandelbrot16_t19_raw_radius_finished)
        {
            "fix-t15-math-payload-or-final-store"
        } else if mandelbrot16_probe_readback_ok {
            "fix-t15-submit-or-eot"
        } else if mandelbrot16_probe_finished {
            "fix-simd16-q12-address-or-store-readback"
        } else {
            "fix-simd16-q12-submit-or-eot"
        };
        crate::log!(
            "mandelbrot-gpu-sidequest: stage-parked reason={} legacy_row_writer_running=0 frame_loop_running=0 obsolete_row_color_protocol=0 artifact_contract=simd16-q12-fixed10-escape-gradient-primary-plane q12_c_re=0x{:08X} q12_c_im=0x{:08X} expected_q12_re1=0x{:08X} expected_plane_value=0x{:08X} next={} does_not_prove=single-heartbeat-full-frame-or-smooth-coloring\n",
            stage_reason,
            MANDELBROT16_BOOT_Q12_C_RE,
            MANDELBROT16_BOOT_Q12_C_IM,
            MANDELBROT16_BOOT_Q12_EXPECTED_RE1,
            MANDELBROT16_BOOT_Q12_EXPECTED_VISIBLE,
            stage_next,
        );
        if MANDELBROT16_T30_FULLSCREEN_ENABLED
            && (mandelbrot16_probe_readback_ok || mandelbrot16_probe_finished)
        {
            let mut control_version_seen = u64::MAX;
            let mut frame_seed = 0x00FF_00FFu32;
            let mut t30_row_cursor = 0u32;
            let mut t30_baseline_row_cursor = 0u32;
            loop {
                Timer::after(EmbassyDuration::from_millis(
                    MANDELBROT_GPGPU_STAGE_READY_HEARTBEAT_MS,
                ))
                .await;
                frame = frame.wrapping_add(1);
                let control = mandelbrot_artifact_control_snapshot();
                let redraw_same_frame = control.version == control_version_seen;
                if !redraw_same_frame {
                    control_version_seen = control.version;
                    frame_seed = 0x00FF_00FF ^ ((control.version as u32) & 0x00FF_FFFF);
                }
                let draw_row = if MANDELBROT16_T30_ADVANCE_ROW_CURSOR {
                    t30_row_cursor
                } else {
                    0
                };
                let fallback_row = if scanout_h > 64 {
                    (scanout_h as u32 / 2).saturating_sub(16)
                } else {
                    draw_row
                };
                let fallback_x = 0;
                let fallback_proof = if MANDELBROT16_T30_IMMEDIATE_LANE0_SWEEP_ENABLED {
                    crate::intel::submit_gpgpu_primary_scanout_mandelbrot16_simd16_t30_immediate_lane0_sweep_bands(
                        frame_seed,
                        draw_row,
                        fallback_x,
                        MANDELBROT16_T30_ROWS_PER_SUBMIT,
                        MANDELBROT16_T30_BANDS_PER_HEARTBEAT,
                        MANDELBROT16_T30_IMMEDIATE_X_BLOCKS_PER_HEARTBEAT,
                        MANDELBROT16_T30_IMMEDIATE_LANE_PHASES_PER_BLOCK,
                    )
                } else {
                    crate::intel::submit_gpgpu_primary_scanout_mandelbrot16_simd16_t30_fullscreen_bands(
                        frame_seed,
                        draw_row,
                        MANDELBROT16_T30_ROWS_PER_SUBMIT,
                        MANDELBROT16_T30_BANDS_PER_HEARTBEAT,
                    )
                };
                let (
                    groupid_submitted,
                    groupid_finished,
                    groupid_readback_ok,
                    groupid_sample_match,
                ) = if MANDELBROT16_T30_GROUPID_DIAGNOSTIC_ENABLED {
                    let proof = crate::intel::submit_gpgpu_primary_scanout_mandelbrot16_simd16_t30_fullscreen_bands(
                        frame_seed,
                        draw_row,
                        MANDELBROT16_T30_ROWS_PER_SUBMIT,
                        MANDELBROT16_T30_BANDS_PER_HEARTBEAT,
                    );
                    (
                        proof.submitted as u8,
                        proof.finished as u8,
                        proof.readback_ok as u8,
                        (proof.output_first_after == proof.sentinel) as u8,
                    )
                } else {
                    (0, 0, 0, 0)
                };
                let (
                    baseline_submitted,
                    baseline_finished,
                    baseline_readback_ok,
                    baseline_first_row,
                    baseline_next_row,
                    baseline_gpu,
                    baseline_sample_before,
                    baseline_sample_after,
                    baseline_sample_expected,
                    baseline_dispatch_delta,
                    baseline_finish_marker,
                ) = if MANDELBROT16_T30_BASELINE_HALFSCREEN_ENABLED {
                    let baseline_height = scanout_h as u32;
                    let baseline_first_row = t30_baseline_row_cursor;
                    let baseline_rows = core::cmp::min(
                        MANDELBROT16_T30_BASELINE_HALFSCREEN_ROWS_PER_HEARTBEAT,
                        baseline_height.saturating_sub(baseline_first_row).max(1),
                    );
                    let mut baseline_submitted_rows = 0u32;
                    let mut baseline_finished_rows = 0u32;
                    let mut baseline_readback_rows = 0u32;
                    let mut baseline_proof = crate::intel::GpgpuOneTileSentinelProof {
                        submitted: false,
                        finished: false,
                        readback_ok: false,
                        reason: "row2560-red-control-not-run",
                        program_name: "row2560-red-control-not-run",
                        output_gpu: 0,
                        sentinel: MANDELBROT16_T30_BASELINE_HALFSCREEN_COLOR,
                        output_first_before: 0,
                        output_first_after: 0,
                        output_nonzero_before: 0,
                        output_nonzero_after: 0,
                        output_hits_lo64: 0,
                        dispatch_delta: 0,
                        finish_marker: 0,
                        expected_finish_marker: 0,
                        batch_bytes: 0,
                    };
                    let mut row_delta = 0u32;
                    while row_delta < baseline_rows {
                        let proof =
                            crate::intel::submit_gpgpu_primary_scanout_row2560_simd8_color_probe(
                                baseline_first_row.saturating_add(row_delta),
                                MANDELBROT16_T30_BASELINE_HALFSCREEN_COLOR,
                                2,
                            );
                        if row_delta == 0 || proof.readback_ok {
                            baseline_proof = proof;
                        }
                        baseline_submitted_rows =
                            baseline_submitted_rows.saturating_add(proof.submitted as u32);
                        baseline_finished_rows =
                            baseline_finished_rows.saturating_add(proof.finished as u32);
                        baseline_readback_rows =
                            baseline_readback_rows.saturating_add(proof.readback_ok as u32);
                        if !proof.finished {
                            break;
                        }
                        row_delta = row_delta.saturating_add(1);
                    }
                    if baseline_readback_rows == baseline_rows {
                        t30_baseline_row_cursor =
                            t30_baseline_row_cursor.saturating_add(baseline_rows);
                        if t30_baseline_row_cursor >= baseline_height {
                            t30_baseline_row_cursor = 0;
                        }
                    }
                    (
                        (baseline_submitted_rows != 0) as u8,
                        (baseline_finished_rows == baseline_rows) as u8,
                        (baseline_readback_rows == baseline_rows) as u8,
                        baseline_first_row,
                        t30_baseline_row_cursor,
                        baseline_proof.output_gpu,
                        baseline_proof.output_first_before,
                        baseline_proof.output_first_after,
                        baseline_proof.sentinel,
                        baseline_proof.dispatch_delta,
                        baseline_proof.finish_marker,
                    )
                } else {
                    (0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0)
                };
                let t31_probe_enabled = MANDELBROT16_T31_STORE_LADDER_ENABLED
                    && (MANDELBROT16_T31_STORE_LADDER_PERIOD_HEARTBEATS == 0
                        || frame % MANDELBROT16_T31_STORE_LADDER_PERIOD_HEARTBEATS == 1);
                let t31_probe = if t31_probe_enabled {
                    let probe_row = fallback_row;
                    let probe_color =
                        0xFF31_0000 | (((frame as u32) & 0xFF) << 8) | (draw_row & 0xFF);
                    crate::log!(
                        "mandelbrot-gpu-sidequest: t31-store-ladder-pre-submit heartbeat={} enabled=1 row={} x={} color=0x{:08X} purpose=last-breadcrumb-before-periodic-safe-store-scoreboard\n",
                        frame,
                        probe_row,
                        fallback_x,
                        probe_color,
                    );
                    Some(crate::intel::submit_gpgpu_primary_scanout_mandelbrot16_simd16_t31_store_ladder_probe(
                        frame,
                        probe_row,
                        fallback_x,
                        probe_color,
                    ))
                } else {
                    None
                };
                let t32_probe = if t31_probe_enabled && MANDELBROT16_T32_SINGLE_SEND_PROBE_ENABLED {
                    let probe_row = fallback_row
                        .saturating_add(1)
                        .min(scanout_h.saturating_sub(1));
                    let probe_color =
                        0xFF32_0000 | (((frame as u32) & 0xFF) << 8) | (draw_row & 0xFF);
                    crate::log!(
                        "mandelbrot-gpu-sidequest: t32-single-send-pre-submit heartbeat={} enabled=1 row={} x={} color=0x{:08X} purpose=last-breadcrumb-before-risky-original-send16-descriptor\n",
                        frame,
                        probe_row,
                        fallback_x,
                        probe_color,
                    );
                    Some(crate::intel::submit_gpgpu_primary_scanout_mandelbrot16_simd16_t32_single_send_probe(
                        frame,
                        probe_row,
                        fallback_x,
                        probe_color,
                    ))
                } else {
                    None
                };
                let t33_probe = if t31_probe_enabled && MANDELBROT16_T33_BTI1_UNTYPED_PROBE_ENABLED
                {
                    let probe_row = fallback_row
                        .saturating_add(2)
                        .min(scanout_h.saturating_sub(1));
                    let probe_color =
                        0xFF33_0000 | (((frame as u32) & 0xFF) << 8) | (draw_row & 0xFF);
                    crate::log!(
                        "mandelbrot-gpu-sidequest: t33-bti1-untyped-pre-submit heartbeat={} enabled=1 row={} x={} color=0x{:08X} purpose=last-breadcrumb-before-bti1-untyped-two-send-store-scoreboard\n",
                        frame,
                        probe_row,
                        fallback_x,
                        probe_color,
                    );
                    Some(crate::intel::submit_gpgpu_primary_scanout_mandelbrot16_simd16_t33_bti1_untyped_probe(
                        frame,
                        probe_row,
                        fallback_x,
                        probe_color,
                    ))
                } else {
                    None
                };
                let t34_probe = if t31_probe_enabled
                    && MANDELBROT16_T34_ADDRESS_DATA_WITNESS_ENABLED
                {
                    let probe_row = fallback_row
                        .saturating_add(3)
                        .min(scanout_h.saturating_sub(1));
                    let color_mask = 0xFF34_0000 | (((frame as u32) & 0xFF) << 8);
                    crate::log!(
                        "mandelbrot-gpu-sidequest: t34-address-data-witness-pre-submit heartbeat={} enabled=1 row={} x={} color_mask=0x{:08X} purpose=last-breadcrumb-before-address-derived-data-scoreboard\n",
                        frame,
                        probe_row,
                        fallback_x,
                        color_mask,
                    );
                    Some(crate::intel::submit_gpgpu_primary_scanout_mandelbrot16_simd16_t34_address_data_witness_probe(
                            frame,
                            probe_row,
                            fallback_x,
                            color_mask,
                        ))
                } else {
                    None
                };
                let t35_probe = if t31_probe_enabled
                    && MANDELBROT16_T35_EXPLICIT_WIDE_PAYLOAD_ENABLED
                {
                    let probe_row = fallback_row
                        .saturating_add(4)
                        .min(scanout_h.saturating_sub(1));
                    let probe_color =
                        0xFF35_0000 | (((frame as u32) & 0xFF) << 8) | (draw_row & 0xFF);
                    crate::log!(
                        "mandelbrot-gpu-sidequest: t35-explicit-wide-payload-pre-submit heartbeat={} enabled=1 row={} x={} color=0x{:08X} purpose=last-breadcrumb-before-explicit-g21-g22-g23-payload-scoreboard\n",
                        frame,
                        probe_row,
                        fallback_x,
                        probe_color,
                    );
                    Some(crate::intel::submit_gpgpu_primary_scanout_mandelbrot16_simd16_t35_explicit_wide_payload_probe(
                            frame,
                            probe_row,
                            fallback_x,
                            probe_color,
                        ))
                } else {
                    None
                };
                let t36_probe = if t31_probe_enabled && MANDELBROT16_T36_UNROLLED_SCALAR16_ENABLED {
                    let probe_row = fallback_row
                        .saturating_add(5)
                        .min(scanout_h.saturating_sub(1));
                    let probe_color =
                        0xFF36_0000 | (((frame as u32) & 0xFF) << 8) | (draw_row & 0xFF);
                    crate::log!(
                        "mandelbrot-gpu-sidequest: t36-unrolled-scalar16-pre-submit heartbeat={} enabled=1 row={} x={} color=0x{:08X} purpose=last-breadcrumb-before-sixteen-scalar-hdc-store-scoreboard\n",
                        frame,
                        probe_row,
                        fallback_x,
                        probe_color,
                    );
                    Some(crate::intel::submit_gpgpu_primary_scanout_mandelbrot16_simd16_t36_unrolled_scalar16_probe(
                            frame,
                            probe_row,
                            fallback_x,
                            probe_color,
                        ))
                } else {
                    None
                };
                let t37_probe = if MANDELBROT16_T37_GROUPID_X_WITNESS_ENABLED {
                    let probe_row = fallback_row;
                    let probe_color =
                        0xFF37_0000 | (((frame as u32) & 0xFF) << 8) | (draw_row & 0xFF);
                    Some(crate::intel::submit_gpgpu_primary_scanout_mandelbrot16_simd16_t37_groupid_x_unrolled_scalar16_probe(
                        frame,
                        probe_row,
                        fallback_x,
                        probe_color,
                        4,
                    ))
                } else {
                    None
                };
                let active_proof = fallback_proof;
                let expected_first = active_proof.sentinel;
                if MANDELBROT16_T30_ADVANCE_ROW_CURSOR && active_proof.finished {
                    let advanced_rows = MANDELBROT16_T30_ROWS_PER_SUBMIT
                        .saturating_mul(MANDELBROT16_T30_BANDS_PER_HEARTBEAT);
                    t30_row_cursor = t30_row_cursor.saturating_add(advanced_rows);
                    if t30_row_cursor >= scanout_h as u32 {
                        t30_row_cursor = 0;
                    }
                }
                crate::log!(
                    "mandelbrot-gpu-sidequest: stage-ready-t30-fullscreen-redraw heartbeat={} reason=simd16-t30-promoted-t38-wide-stamp-fill artifact_stage=simd16-t30-t38-wide-stamp-primary-plane color_mode=xy-gradient-runtime-patched-block-constant runtime_control=draw-same-frame-again redraw_same_frame={} control_version={} frame_seed=0x{:08X} target={}x{} row_cursor_enabled={} draw_row={} fallback_row={} fallback_x={} next_row_cursor={} rows_per_submit={} bands_per_heartbeat={} immediate_x_blocks_per_heartbeat={} immediate_lane_phases_per_block={} immediate_lane0_enabled={} submitted={} finished={} readback_ok={} sample_gpu=0x{:X} sample_before=0x{:08X} sample_after=0x{:08X} sample_expected=0x{:08X} sample_match={} lane_dispatch_delta={} finish_marker=0x{:08X} groupid_diagnostic_enabled={} groupid_submitted={} groupid_finished={} groupid_readback_ok={} groupid_sample_match={} active_t38_wide_stamp_fill={} active_finished={} active_sample_match={} next=raise-rows-per-heartbeat-or-prove-gpu-side-groupid-xy proves=t30-runtime-loop-uses-t38-wide-stamp-contract does_not_prove=smooth-mandelbrot-coloring-or-gpu-side-coordinate-generation\n",
                    frame,
                    redraw_same_frame as u8,
                    control.version,
                    frame_seed,
                    scanout_w,
                    scanout_h,
                    MANDELBROT16_T30_ADVANCE_ROW_CURSOR as u8,
                    draw_row,
                    fallback_row,
                    fallback_x,
                    t30_row_cursor,
                    MANDELBROT16_T30_ROWS_PER_SUBMIT,
                    MANDELBROT16_T30_BANDS_PER_HEARTBEAT,
                    MANDELBROT16_T30_IMMEDIATE_X_BLOCKS_PER_HEARTBEAT,
                    MANDELBROT16_T30_IMMEDIATE_LANE_PHASES_PER_BLOCK,
                    MANDELBROT16_T30_IMMEDIATE_LANE0_SWEEP_ENABLED as u8,
                    active_proof.submitted as u8,
                    active_proof.finished as u8,
                    active_proof.readback_ok as u8,
                    active_proof.output_gpu,
                    active_proof.output_first_before,
                    active_proof.output_first_after,
                    expected_first,
                    (active_proof.output_first_after == expected_first) as u8,
                    active_proof.dispatch_delta,
                    active_proof.finish_marker,
                    MANDELBROT16_T30_GROUPID_DIAGNOSTIC_ENABLED as u8,
                    groupid_submitted,
                    groupid_finished,
                    groupid_readback_ok,
                    groupid_sample_match,
                    1u8,
                    fallback_proof.finished as u8,
                    (fallback_proof.output_first_after == expected_first) as u8,
                );
                if MANDELBROT16_T30_BASELINE_HALFSCREEN_ENABLED {
                    crate::log!(
                        "mandelbrot-gpu-sidequest: t30-baseline-halfscreen-red heartbeat={} enabled=1 rect={}x{}@{},{} first_row={} next_row={} rows_per_heartbeat={} color=0x{:08X} submitted={} finished={} readback_ok={} output_gpu=0x{:X} sample_before=0x{:08X} sample_after=0x{:08X} sample_expected=0x{:08X} sample_match={} lane_dispatch_delta={} finish_marker=0x{:08X} purpose=separate-scanout-rectangle-control-test proves=runtime-can-ask-existing-row-artifact-for-large-visible-red-region does_not_prove=mandelbrot16-simd16-all-lanes-store\n",
                        frame,
                        scanout_w,
                        scanout_h,
                        0,
                        0,
                        baseline_first_row,
                        baseline_next_row,
                        MANDELBROT16_T30_BASELINE_HALFSCREEN_ROWS_PER_HEARTBEAT,
                        MANDELBROT16_T30_BASELINE_HALFSCREEN_COLOR,
                        baseline_submitted,
                        baseline_finished,
                        baseline_readback_ok,
                        baseline_gpu,
                        baseline_sample_before,
                        baseline_sample_after,
                        baseline_sample_expected,
                        (baseline_sample_after == baseline_sample_expected) as u8,
                        baseline_dispatch_delta,
                        baseline_finish_marker,
                    );
                }
                if let Some(t31_probe) = t31_probe {
                    crate::log!(
                        "mandelbrot-gpu-sidequest: t31-store-ladder heartbeat={} enabled=1 row={} x={} submitted={} finished={} all_lane_readback_ok={} sample_gpu=0x{:X} sample_before=0x{:08X} sample_after=0x{:08X} sample_expected=0x{:08X} hit_mask=0x{:04X} pass_lane0={} pass_lane01={} pass_lane03={} pass_lane07={} pass_lane15={} lane_dispatch_delta={} finish_marker=0x{:08X} purpose=periodic-simd16-store-collapse-scoreboard next=if-only-lane0-then-swap-send-descriptor-and-payload-layout does_not_prove=mandelbrot-math-or-fullscreen-performance\n",
                        frame,
                        fallback_row,
                        fallback_x,
                        t31_probe.submitted as u8,
                        t31_probe.finished as u8,
                        t31_probe.readback_ok as u8,
                        t31_probe.output_gpu,
                        t31_probe.output_first_before,
                        t31_probe.output_first_after,
                        t31_probe.sentinel,
                        t31_probe.output_hits_lo64 as u16,
                        ((t31_probe.output_hits_lo64 & 0x0001) == 0x0001) as u8,
                        ((t31_probe.output_hits_lo64 & 0x0003) == 0x0003) as u8,
                        ((t31_probe.output_hits_lo64 & 0x000F) == 0x000F) as u8,
                        ((t31_probe.output_hits_lo64 & 0x00FF) == 0x00FF) as u8,
                        ((t31_probe.output_hits_lo64 & 0xFFFF) == 0xFFFF) as u8,
                        t31_probe.dispatch_delta,
                        t31_probe.finish_marker,
                    );
                }
                if let Some(t32_probe) = t32_probe {
                    crate::log!(
                        "mandelbrot-gpu-sidequest: t32-single-send heartbeat={} enabled=1 row={} x={} submitted={} finished={} all_lane_readback_ok={} sample_gpu=0x{:X} sample_before=0x{:08X} sample_after=0x{:08X} sample_expected=0x{:08X} hit_mask=0x{:04X} pass_lane0={} pass_lane01={} pass_lane03={} pass_lane07={} pass_lane15={} lane_dispatch_delta={} finish_marker=0x{:08X} purpose=compare-original-send16-descriptor-with-t31-two-send-path next=if-wide-promote-single-send-constant-body-to-t30 else-explicitly-materialize-g21-g23 does_not_prove=mandelbrot-math-or-groupid-linear-address\n",
                        frame,
                        fallback_row
                            .saturating_add(1)
                            .min(scanout_h.saturating_sub(1)),
                        fallback_x,
                        t32_probe.submitted as u8,
                        t32_probe.finished as u8,
                        t32_probe.readback_ok as u8,
                        t32_probe.output_gpu,
                        t32_probe.output_first_before,
                        t32_probe.output_first_after,
                        t32_probe.sentinel,
                        t32_probe.output_hits_lo64 as u16,
                        ((t32_probe.output_hits_lo64 & 0x0001) == 0x0001) as u8,
                        ((t32_probe.output_hits_lo64 & 0x0003) == 0x0003) as u8,
                        ((t32_probe.output_hits_lo64 & 0x000F) == 0x000F) as u8,
                        ((t32_probe.output_hits_lo64 & 0x00FF) == 0x00FF) as u8,
                        ((t32_probe.output_hits_lo64 & 0xFFFF) == 0xFFFF) as u8,
                        t32_probe.dispatch_delta,
                        t32_probe.finish_marker,
                    );
                }
                if let Some(t33_probe) = t33_probe {
                    crate::log!(
                        "mandelbrot-gpu-sidequest: t33-bti1-untyped heartbeat={} enabled=1 row={} x={} submitted={} finished={} all_lane_readback_ok={} sample_gpu=0x{:X} sample_before=0x{:08X} sample_after=0x{:08X} sample_expected=0x{:08X} hit_mask=0x{:04X} pass_lane0={} pass_lane01={} pass_lane03={} pass_lane07={} pass_lane15={} lane_dispatch_delta={} finish_marker=0x{:08X} purpose=compare-proven-bti1-untyped-descriptor-with-legacy-stateless-t31 next=if-wide-promote-bti1-untyped-to-t30 else-fix-lane-address-materialization does_not_prove=mandelbrot-math-or-groupid-linear-address\n",
                        frame,
                        fallback_row
                            .saturating_add(2)
                            .min(scanout_h.saturating_sub(1)),
                        fallback_x,
                        t33_probe.submitted as u8,
                        t33_probe.finished as u8,
                        t33_probe.readback_ok as u8,
                        t33_probe.output_gpu,
                        t33_probe.output_first_before,
                        t33_probe.output_first_after,
                        t33_probe.sentinel,
                        t33_probe.output_hits_lo64 as u16,
                        ((t33_probe.output_hits_lo64 & 0x0001) == 0x0001) as u8,
                        ((t33_probe.output_hits_lo64 & 0x0003) == 0x0003) as u8,
                        ((t33_probe.output_hits_lo64 & 0x000F) == 0x000F) as u8,
                        ((t33_probe.output_hits_lo64 & 0x00FF) == 0x00FF) as u8,
                        ((t33_probe.output_hits_lo64 & 0xFFFF) == 0xFFFF) as u8,
                        t33_probe.dispatch_delta,
                        t33_probe.finish_marker,
                    );
                }
                if let Some(t34_probe) = t34_probe {
                    let t34_lane0_only = (t34_probe.output_hits_lo64 & 0x0001) == 0x0001
                        && (t34_probe.output_hits_lo64 & 0xFFFE) == 0
                        && t34_probe.output_first_after == t34_probe.sentinel;
                    let t34_alias_or_late_lane = t34_probe.output_first_after
                        != t34_probe.output_first_before
                        && t34_probe.output_first_after != t34_probe.sentinel;
                    crate::log!(
                        "mandelbrot-gpu-sidequest: t34-address-data-witness heartbeat={} enabled=1 row={} x={} submitted={} finished={} all_lane_readback_ok={} sample_gpu=0x{:X} sample_before=0x{:08X} sample_after=0x{:08X} lane0_expected=0x{:08X} hit_mask=0x{:04X} lane0_only={} alias_or_late_lane={} lane_dispatch_delta={} finish_marker=0x{:08X} purpose=classify-legacy-simd16-store-collapse next=if-lane0-only-build-explicit-wide-send-payload else-fix-address-vector does_not_prove=mandelbrot-math-or-fullscreen-performance\n",
                        frame,
                        fallback_row
                            .saturating_add(3)
                            .min(scanout_h.saturating_sub(1)),
                        fallback_x,
                        t34_probe.submitted as u8,
                        t34_probe.finished as u8,
                        t34_probe.readback_ok as u8,
                        t34_probe.output_gpu,
                        t34_probe.output_first_before,
                        t34_probe.output_first_after,
                        t34_probe.sentinel,
                        t34_probe.output_hits_lo64 as u16,
                        t34_lane0_only as u8,
                        t34_alias_or_late_lane as u8,
                        t34_probe.dispatch_delta,
                        t34_probe.finish_marker,
                    );
                }
                if let Some(t35_probe) = t35_probe {
                    let t35_hit_mask = t35_probe.output_hits_lo64 as u16;
                    crate::log!(
                        "mandelbrot-gpu-sidequest: t35-explicit-wide-payload heartbeat={} enabled=1 row={} x={} submitted={} finished={} all_lane_readback_ok={} sample_gpu=0x{:X} sample_before=0x{:08X} sample_after=0x{:08X} sample_expected=0x{:08X} hit_mask=0x{:04X} pass_lane0={} pass_low8={} pass_lane8={} pass_high8={} pass_lane15={} lane_dispatch_delta={} finish_marker=0x{:08X} purpose=classify-explicit-wide-message-payload next=if-only-lane0-then-rebuild-send-descriptor-contract-if-low8-or-high8-then-promote-that-half does_not_prove=mandelbrot-math-or-groupid-linear-address\n",
                        frame,
                        fallback_row
                            .saturating_add(4)
                            .min(scanout_h.saturating_sub(1)),
                        fallback_x,
                        t35_probe.submitted as u8,
                        t35_probe.finished as u8,
                        t35_probe.readback_ok as u8,
                        t35_probe.output_gpu,
                        t35_probe.output_first_before,
                        t35_probe.output_first_after,
                        t35_probe.sentinel,
                        t35_hit_mask,
                        ((t35_hit_mask & 0x0001) == 0x0001) as u8,
                        ((t35_hit_mask & 0x00FF) == 0x00FF) as u8,
                        ((t35_hit_mask & 0x0100) == 0x0100) as u8,
                        ((t35_hit_mask & 0xFF00) == 0xFF00) as u8,
                        ((t35_hit_mask & 0x8000) == 0x8000) as u8,
                        t35_probe.dispatch_delta,
                        t35_probe.finish_marker,
                    );
                }
                if let Some(t36_probe) = t36_probe {
                    crate::log!(
                        "mandelbrot-gpu-sidequest: t36-unrolled-scalar16 heartbeat={} enabled=1 row={} x={} submitted={} finished={} all_lane_readback_ok={} sample_gpu=0x{:X} sample_before=0x{:08X} sample_after=0x{:08X} sample_expected=0x{:08X} hit_mask=0x{:04X} pass_lane0={} pass_lane01={} pass_lane03={} pass_lane07={} pass_lane15={} lane_dispatch_delta={} finish_marker=0x{:08X} purpose=prove-one-eu-invocation-can-fill-16-pixel-block-with-scalar-sends next=if-ffff-promote-to-t30-block-fill does_not_prove=mandelbrot-math-or-groupid-linear-address\n",
                        frame,
                        fallback_row
                            .saturating_add(5)
                            .min(scanout_h.saturating_sub(1)),
                        fallback_x,
                        t36_probe.submitted as u8,
                        t36_probe.finished as u8,
                        t36_probe.readback_ok as u8,
                        t36_probe.output_gpu,
                        t36_probe.output_first_before,
                        t36_probe.output_first_after,
                        t36_probe.sentinel,
                        t36_probe.output_hits_lo64 as u16,
                        ((t36_probe.output_hits_lo64 & 0x0001) == 0x0001) as u8,
                        ((t36_probe.output_hits_lo64 & 0x0003) == 0x0003) as u8,
                        ((t36_probe.output_hits_lo64 & 0x000F) == 0x000F) as u8,
                        ((t36_probe.output_hits_lo64 & 0x00FF) == 0x00FF) as u8,
                        ((t36_probe.output_hits_lo64 & 0xFFFF) == 0xFFFF) as u8,
                        t36_probe.dispatch_delta,
                        t36_probe.finish_marker,
                    );
                }
                if let Some(t37_probe) = t37_probe {
                    crate::log!(
                        "mandelbrot-gpu-sidequest: t37-groupid-x-witness heartbeat={} enabled=1 row={} x={} row_groups=4 submitted={} finished={} groupid_x_readback_ok={} sample_gpu=0x{:X} sample_before=0x{:08X} sample_after=0x{:08X} sample_expected=0x{:08X} first_block_hit_mask=0x{:04X} pass_first_block_all_lanes={} lane_dispatch_delta={} finish_marker=0x{:08X} purpose=prove-gpu-side-groupid-x-selects-adjacent-16px-blocks next=if-ok-promote-t30-to-row-group-walker else-fix-r0-groupid-source-prelude does_not_prove=groupid-y-or-mandelbrot-math\n",
                        frame,
                        fallback_row,
                        fallback_x,
                        t37_probe.submitted as u8,
                        t37_probe.finished as u8,
                        t37_probe.readback_ok as u8,
                        t37_probe.output_gpu,
                        t37_probe.output_first_before,
                        t37_probe.output_first_after,
                        t37_probe.sentinel,
                        t37_probe.output_hits_lo64 as u16,
                        ((t37_probe.output_hits_lo64 & 0xFFFF) == 0xFFFF) as u8,
                        t37_probe.dispatch_delta,
                        t37_probe.finish_marker,
                    );
                }
            }
        }
        let sweep_x_groups = (scanout_w as u32).saturating_add(
            trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_PIXELS_PER_PROGRAM as u32 - 1,
        )
            / trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_PIXELS_PER_PROGRAM as u32;
        let sweep_y_blocks = (scanout_h as u32)
            .saturating_add(MANDELBROT16_BOOT_VISIBLE_STAMP_ROWS - 1)
            / MANDELBROT16_BOOT_VISIBLE_STAMP_ROWS;
        let sweep_blocks = sweep_x_groups.saturating_mul(sweep_y_blocks).max(1);
        let mut sweep_y_cursor = 0u32;
        let mut fixed10_sweep_alive = t15_sweep_gate;
        let mut fixed10_sweep_stop_logged = false;
        let mut fixed10_row_group_scale_index = 0usize;
        let mut fixed10_x_block_scale_index = 0usize;
        loop {
            Timer::after(EmbassyDuration::from_millis(MANDELBROT_GPGPU_STAGE_READY_HEARTBEAT_MS))
                .await;
            frame = frame.wrapping_add(1);
            let sweep_live = fixed10_sweep_alive && MANDELBROT16_STAGE_READY_SWEEP_ENABLED;
            if frame == 1 || sweep_live || frame % 16 == 0 {
                let mut restamped_rows = 0u32;
                let mut restamped_x_blocks = 0u32;
                let mut restamp_finish_marker = 0u32;
                let mut restamp_dispatch_delta = 0u64;
                let mut restamp_stopped = false;
                let mut sample_observed = 0u32;
                let mut sample_expected = 0u32;
                let mut sample_gpu = 0u64;
                let mut sample_valid = false;
                let mut x_base: u32;
                let mut row_index = ((scanout_h as u32) / 2).saturating_sub(16);
                let mut sweep_block = 0u32;
                let mut sweep_x_group = sweep_x_groups / 2;
                let mut sweep_y_block = sweep_y_blocks / 2;
                if sweep_live {
                    sweep_y_block = if sweep_y_blocks == 0 {
                        0
                    } else {
                        sweep_y_cursor % sweep_y_blocks
                    };
                    sweep_block = sweep_y_block.saturating_mul(sweep_x_groups);
                    row_index = sweep_y_block.saturating_mul(MANDELBROT16_BOOT_VISIBLE_STAMP_ROWS);
                }
                if mandelbrot16_probe_readback_ok && fixed10_sweep_alive {
                    let submitted_x_block_scale_index = fixed10_x_block_scale_index;
                    let scaled_x_blocks = MANDELBROT16_STAGE_READY_X_BLOCK_SCALE
                        [submitted_x_block_scale_index]
                        .max(1)
                        .min(sweep_x_groups.max(1));
                    let x_blocks_this_heartbeat = if sweep_live {
                        MANDELBROT16_STAGE_READY_SWEEP_X_BLOCKS_PER_HEARTBEAT
                            .max(1)
                            .max(scaled_x_blocks)
                            .min(sweep_x_groups.max(1))
                    } else {
                        1
                    };
                    let submitted_row_group_scale_index = fixed10_row_group_scale_index;
                    let submitted_row_groups = MANDELBROT16_STAGE_READY_ROW_GROUP_SCALE
                        [submitted_row_group_scale_index]
                        .clamp(1, MANDELBROT16_BOOT_VISIBLE_STAMP_ROWS);
                    let t11_linear_band = submitted_row_group_scale_index + 1
                        == MANDELBROT16_STAGE_READY_ROW_GROUP_SCALE.len()
                        && submitted_x_block_scale_index + 1
                            == MANDELBROT16_STAGE_READY_X_BLOCK_SCALE.len();
                    let mut submit_calls = 0u32;
                    if sweep_live {
                        sweep_x_group = x_blocks_this_heartbeat.saturating_sub(1);
                        x_base = 0;
                        let total_linear_groups = if t15_linear_address_ready {
                            submitted_row_groups.saturating_mul(x_blocks_this_heartbeat)
                        } else {
                            x_blocks_this_heartbeat
                        };
                        let mut linear_group_base = 0u32;
                        while linear_group_base < total_linear_groups {
                            let linear_groups_this_submit = if t15_linear_address_ready {
                                MANDELBROT16_T11_LINEAR_MAX_GROUPS_PER_SUBMIT
                                    .min(total_linear_groups.saturating_sub(linear_group_base))
                            } else {
                                1
                            };
                            let linear_pixel_base = linear_group_base.saturating_mul(
                                trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_PIXELS_PER_PROGRAM
                                    as u32,
                            );
                            let chunk_x_group = if sweep_x_groups == 0 {
                                0
                            } else {
                                linear_group_base % sweep_x_groups
                            };
                            let chunk_y_block = if sweep_x_groups == 0 {
                                sweep_y_block
                            } else {
                                sweep_y_block
                                    .saturating_add(linear_group_base / sweep_x_groups)
                                    .min(sweep_y_blocks.saturating_sub(1))
                            };
                            let c_re = mandelbrot16_pixel_c_re_q12(
                                chunk_x_group.saturating_mul(
                                    trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_PIXELS_PER_PROGRAM
                                        as u32,
                                ),
                            );
                            let c_im = mandelbrot16_block_c_im_q12(chunk_y_block, sweep_y_blocks);
                            let submit_x_base = if t15_linear_address_ready {
                                linear_pixel_base
                            } else {
                                chunk_x_group.saturating_mul(
                                    trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_PIXELS_PER_PROGRAM
                                        as u32,
                                )
                            };
                            let submit_row_index = if t15_linear_address_ready {
                                row_index
                            } else {
                                chunk_y_block.saturating_mul(MANDELBROT16_BOOT_VISIBLE_STAMP_ROWS)
                            };
                            let row_probe = if t15_linear_address_ready {
                                crate::intel::submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_quiet_linear_band(
                                    row_index,
                                    linear_pixel_base,
                                    1,
                                    linear_groups_this_submit,
                                    c_re,
                                    c_im,
                                )
                            } else if t19_raw_radius_ready {
                                crate::intel::submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_quiet_immediate_raw_radius_rows(
                                    submit_row_index,
                                    submit_x_base,
                                    submitted_row_groups,
                                    c_re,
                                    c_im,
                                )
                            } else {
                                let color = mandelbrot16_visible_constant_color(
                                    frame,
                                    chunk_x_group,
                                    chunk_y_block,
                                );
                                crate::intel::submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_quiet_immediate_constant(
                                    submit_row_index,
                                    submit_x_base,
                                    color,
                                )
                            };
                            restamp_finish_marker = row_probe.finish_marker;
                            restamp_dispatch_delta =
                                restamp_dispatch_delta.saturating_add(row_probe.dispatch_delta);
                            submit_calls = submit_calls.saturating_add(1);
                            if !sample_valid {
                                sample_observed = row_probe.output_first_after;
                                sample_expected = row_probe.sentinel;
                                sample_gpu = row_probe.output_gpu;
                                sample_valid = row_probe.submitted;
                            }
                            if !row_probe.finished {
                                fixed10_sweep_alive = false;
                                restamp_stopped = true;
                                x_base = submit_x_base;
                                break;
                            }
                            linear_group_base =
                                linear_group_base.saturating_add(linear_groups_this_submit);
                        }
                        if !restamp_stopped {
                            restamped_rows = restamped_rows.saturating_add(
                                submitted_row_groups.saturating_mul(x_blocks_this_heartbeat),
                            );
                            restamped_x_blocks =
                                restamped_x_blocks.saturating_add(x_blocks_this_heartbeat);
                        }
                    } else {
                        x_base = sweep_x_group.saturating_mul(
                            trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_PIXELS_PER_PROGRAM
                                as u32,
                        );
                        let c_re = mandelbrot16_pixel_c_re_q12(x_base);
                        let c_im = mandelbrot16_block_c_im_q12(sweep_y_block, sweep_y_blocks);
                        let row_probe = if t15_linear_address_ready {
                            crate::intel::submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_quiet_linear_band(
                                row_index,
                                x_base,
                                1,
                                1,
                                c_re,
                                c_im,
                            )
                        } else if t19_raw_radius_ready {
                            crate::intel::submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_quiet_immediate_raw_radius(
                                row_index,
                                x_base,
                                c_re,
                                c_im,
                            )
                        } else {
                            let color = mandelbrot16_visible_constant_color(
                                frame,
                                sweep_x_group,
                                sweep_y_block,
                            );
                            crate::intel::submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_quiet_immediate_constant(
                                row_index,
                                x_base,
                                color,
                            )
                        };
                        if row_probe.finished {
                            restamped_rows = restamped_rows.saturating_add(1);
                            restamped_x_blocks = restamped_x_blocks.saturating_add(1);
                        }
                        restamp_finish_marker = row_probe.finish_marker;
                        restamp_dispatch_delta =
                            restamp_dispatch_delta.saturating_add(row_probe.dispatch_delta);
                        submit_calls = submit_calls.saturating_add(1);
                        if !sample_valid {
                            sample_observed = row_probe.output_first_after;
                            sample_expected = row_probe.sentinel;
                            sample_gpu = row_probe.output_gpu;
                            sample_valid = row_probe.submitted;
                        }
                        if !row_probe.finished {
                            fixed10_sweep_alive = false;
                            restamp_stopped = true;
                        }
                    }
                    if sweep_live && fixed10_sweep_alive {
                        if fixed10_row_group_scale_index + 1
                            < MANDELBROT16_STAGE_READY_ROW_GROUP_SCALE.len()
                        {
                            fixed10_row_group_scale_index =
                                fixed10_row_group_scale_index.saturating_add(1);
                        } else if fixed10_x_block_scale_index + 1
                            < MANDELBROT16_STAGE_READY_X_BLOCK_SCALE.len()
                        {
                            fixed10_x_block_scale_index =
                                fixed10_x_block_scale_index.saturating_add(1);
                        }
                        sweep_y_cursor = if sweep_y_blocks == 0 {
                            0
                        } else {
                            (sweep_y_cursor + 1) % sweep_y_blocks
                        };
                    }
                    crate::log!(
                        "mandelbrot-gpu-sidequest: stage-ready-simd16-visual-sweep-probe heartbeat={} reason={} artifact_stage={} address_path={} sweep_enabled={} sweep_alive={} stopped={} t11_linear_band={} linear_max_groups_per_submit={} submit_calls={} sweep_block={}/{} sweep_y_cursor={} sweep_xy={}x{} sweep_grid={}x{} row_group_scale_index={} x_block_scale_index={} stamp_rect={}x{}@{},{} submitted_row_groups={} submitted_x_blocks={} restamped_x_blocks={} restamped_groups={} per_row_readback=0 sample_valid={} sample_gpu=0x{:X} sample_observed=0x{:08X} sample_expected=0x{:08X} sample_match={} lane_dispatch_delta={} finish_marker=0x{:08X} expected_plane_value=0x{:08X} next={} proves=t17-or-t15-submit-dispatch-eot-and-scanout-sample-instrumentation does_not_prove=sample-match-or-physical-display-visible-or-single-heartbeat-full-frame-or-smooth-coloring\n",
                        frame,
                        stage_reason,
                        if t19_raw_radius_ready {
                            "simd16-t19-immediate-eu-lane-re-row-chunk-ci-fixed1-raw-radius-primary-plane"
                        } else if t15_linear_address_ready {
                            "simd16-t15-linear-eu-lane-re-row-chunk-ci-fixed10-escape-gradient-primary-plane"
                        } else {
                            "simd16-t17-immediate-base-constant-color-primary-plane"
                        },
                        if t15_linear_address_ready {
                            "linear-groupid"
                        } else if t19_raw_radius_ready {
                            "immediate-base-raw-radius-fallback"
                        } else {
                            "immediate-base-fallback"
                        },
                        sweep_live as u8,
                        fixed10_sweep_alive as u8,
                        restamp_stopped as u8,
                        t11_linear_band as u8,
                        MANDELBROT16_T11_LINEAR_MAX_GROUPS_PER_SUBMIT,
                        submit_calls,
                        sweep_block,
                        sweep_blocks,
                        sweep_y_cursor,
                        sweep_x_group,
                        sweep_y_block,
                        sweep_x_groups,
                        sweep_y_blocks,
                        submitted_row_group_scale_index,
                        submitted_x_block_scale_index,
                        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_PIXELS_PER_PROGRAM,
                        submitted_row_groups,
                        x_base,
                        row_index,
                        submitted_row_groups,
                        x_blocks_this_heartbeat,
                        restamped_x_blocks,
                        restamped_rows,
                        sample_valid as u8,
                        sample_gpu,
                        sample_observed,
                        sample_expected,
                        (sample_valid && sample_observed == sample_expected) as u8,
                        restamp_dispatch_delta,
                        restamp_finish_marker,
                        MANDELBROT16_BOOT_Q12_EXPECTED_VISIBLE,
                        stage_next,
                    );
                    if restamp_stopped && !fixed10_sweep_stop_logged {
                        fixed10_sweep_stop_logged = true;
                        crate::log!(
                            "mandelbrot-gpu-sidequest: fixed10-escape-bw-scale-disabled heartbeat={} reason=fixed10-rowgroup-or-xblock-scale-submit-did-not-retire artifact_stage=simd16-q12-fixed10-escape-bw-primary-plane row_group_scale_index={} x_block_scale_index={} submitted_row_groups={} submitted_x_blocks={} failed_xy={}x{} failed_rect_or_linear_span={}x{}@{},{} finish_marker=0x{:08X} lane_dispatch_delta={} action=park-auto-sweep-use-shell-for-single-probes next=fix-groupid-row-address-or-walker-timeout does_not_prove=iteration-count-gradient-or-full-frame-mandelbrot\n",
                            frame,
                            submitted_row_group_scale_index,
                            submitted_x_block_scale_index,
                            submitted_row_groups,
                            x_blocks_this_heartbeat,
                            sweep_x_group,
                            sweep_y_block,
                            trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_PIXELS_PER_PROGRAM,
                            submitted_row_groups,
                            x_base,
                            row_index,
                            restamp_finish_marker,
                            restamp_dispatch_delta,
                        );
                    }
                }
                if !mandelbrot16_probe_readback_ok || !sweep_live || frame % 16 == 0 {
                    crate::log!(
                        "mandelbrot-gpu-sidequest: stage-ready heartbeat={} reason={} artifact_stage={} legacy_row_writer_running=0 obsolete_row_color_protocol=0 restamp_enabled={} sweep_enabled={} sweep_alive={} next={} does_not_prove=single-heartbeat-full-frame-or-smooth-coloring\n",
                        frame,
                        stage_reason,
                        if t15_linear_address_ready {
                            "simd16-t15-linear-eu-lane-re-row-chunk-ci-fixed10-escape-gradient-primary-plane"
                        } else if t19_raw_radius_ready {
                            "simd16-t19-immediate-eu-lane-re-row-chunk-ci-fixed1-raw-radius-primary-plane"
                        } else {
                            "simd16-t17-immediate-base-constant-color-primary-plane"
                        },
                        (mandelbrot16_probe_readback_ok && fixed10_sweep_alive) as u8,
                        MANDELBROT16_STAGE_READY_SWEEP_ENABLED as u8,
                        fixed10_sweep_alive as u8,
                        stage_next,
                    );
                }
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
