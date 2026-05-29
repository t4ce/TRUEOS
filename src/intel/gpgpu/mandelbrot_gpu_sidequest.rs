#![allow(dead_code)]

use embassy_executor::{SpawnError, Spawner};
use embassy_time::{Duration as EmbassyDuration, Timer};

pub(crate) const MANDELBROT_GPU_SIDEQUEST_NAME: &str = "mandelbrot-gpu-sidequest";
pub(crate) const MANDELBROT_TARGET_WIDTH: u32 = 2560;
pub(crate) const MANDELBROT_TARGET_HEIGHT: u32 = 1440;
pub(crate) const MANDELBROT_GPGPU_LOOP_MS: u64 = 16;
pub(crate) const MANDELBROT_GPGPU_PRESENT_FLUSH_BYTES: usize = 0xE10000;
pub(crate) const MANDELBROT16_BOOT_Q12_MODE_ONE_ITER_VISIBLE: u32 = 43;
pub(crate) const MANDELBROT16_T15_MODE_LINEAR_GRADIENT_FULL_BAND: u32 = 47;
pub(crate) const MANDELBROT16_T17_MODE_IMMEDIATE_CONSTANT_STORE: u32 = 49;
pub(crate) const MANDELBROT16_T19_MODE_IMMEDIATE_RAW_RADIUS_STORE: u32 = 51;
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
pub(crate) const MANDELBROT16_SWEEP_X_BLOCKS_PER_FRAME: u32 = 160;
pub(crate) const MANDELBROT16_SWEEP_ROW_GROUPS: u32 = 1;

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
    let pixels_per_program =
        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_PIXELS_PER_PROGRAM as u32;
    let x_base = scanout_w.saturating_sub(pixels_per_program) / 2;
    let row_index = (scanout_h / 2).saturating_sub(16);
    let mut released_lumen = false;

    crate::log!(
        "mandelbrot-gpu-sidequest: start name={} artifact_stage=simd16-q12-fixed10-gradient target={}x{} scanout={}x{} primary_gpu=0x{:X} active_artifact={} retired_artifacts=q12-simd8-preview,legacy-row-writer,scalar-canaries,row2560-diagnostics next=boot-simd16-proof\n",
        MANDELBROT_GPU_SIDEQUEST_NAME,
        MANDELBROT_TARGET_WIDTH,
        MANDELBROT_TARGET_HEIGHT,
        scanout_w,
        scanout_h,
        primary_gpu,
        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_PROGRAM_NAME,
    );

    let boot = crate::intel::submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_probe(
        MANDELBROT16_BOOT_Q12_MODE_ONE_ITER_VISIBLE,
        row_index,
        x_base,
        MANDELBROT16_BOOT_Q12_C_RE,
        MANDELBROT16_BOOT_Q12_C_IM,
    );
    if boot.readback_ok {
        crate::r::readiness::set(crate::r::readiness::MANDELBROT_GPU_SIDEQUEST_READY);
        released_lumen = true;
    }
    crate::log!(
        "mandelbrot-gpu-sidequest: boot-proof submitted={} finished={} readback_ok={} reason={} program_source={} target_gpu=0x{:X} stamp_rect={}x1@{},{} hit_mask=0x{:04X} lane_dispatch_delta={} finish_marker=0x{:08X} q12_c_re=0x{:08X} q12_c_im=0x{:08X} expected_q12_re1=0x{:08X} expected_plane_value=0x{:08X} proves={} next={}\n",
        boot.submitted as u8,
        boot.finished as u8,
        boot.readback_ok as u8,
        boot.reason,
        boot.program_name,
        boot.output_gpu,
        pixels_per_program,
        x_base,
        row_index,
        boot.output_hits_lo64 as u32,
        boot.dispatch_delta,
        boot.finish_marker,
        MANDELBROT16_BOOT_Q12_C_RE,
        MANDELBROT16_BOOT_Q12_C_IM,
        MANDELBROT16_BOOT_Q12_EXPECTED_RE1,
        MANDELBROT16_BOOT_Q12_EXPECTED_VISIBLE,
        if boot.readback_ok {
            "simd16-one-iteration-visible-store"
        } else if boot.dispatch_delta != 0 {
            "simd16-dispatch-with-store-mismatch"
        } else {
            "simd16-submit-attempt"
        },
        if boot.readback_ok {
            "probe-gradient-path"
        } else {
            "park"
        },
    );

    if !boot.readback_ok {
        park("boot-proof-not-ready").await;
    }

    let constant = crate::intel::submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_immediate_constant_probe(
        row_index,
        x_base,
        0xFF33_CC00,
    );
    let row_constant = if constant.readback_ok {
        crate::intel::submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_immediate_constant_rows_probe(
            row_index,
            x_base,
            MANDELBROT16_BOOT_VISIBLE_STAMP_ROWS,
            0xFF33_CC00,
        )
    } else {
        constant
    };
    let linear_constant = crate::intel::submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_linear_constant_probe(
        row_index,
        x_base,
        0xFF33_CC00,
    );
    crate::log!(
        "mandelbrot-gpu-sidequest: address-proof constant_ok={} row_constant_ok={} linear_constant_ok={} constant_finished={} row_finished={} linear_finished={} linear_reason={} row_reason={} next={}\n",
        constant.readback_ok as u8,
        row_constant.readback_ok as u8,
        linear_constant.readback_ok as u8,
        constant.finished as u8,
        row_constant.finished as u8,
        linear_constant.finished as u8,
        linear_constant.reason,
        row_constant.reason,
        if linear_constant.readback_ok {
            "linear-gradient"
        } else if row_constant.readback_ok {
            "immediate-gradient-or-raw-radius"
        } else {
            "constant-fallback"
        },
    );

    let c_re = mandelbrot16_pixel_c_re_q12(x_base);
    let c_im = mandelbrot16_block_c_im_q12(
        row_index / MANDELBROT16_BOOT_VISIBLE_STAMP_ROWS,
        scanout_h.saturating_add(MANDELBROT16_BOOT_VISIBLE_STAMP_ROWS - 1)
            / MANDELBROT16_BOOT_VISIBLE_STAMP_ROWS,
    );
    let gradient = if linear_constant.readback_ok {
        crate::intel::submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_linear_band_probe(
            row_index,
            x_base,
            1,
            1,
            c_re,
            c_im,
        )
    } else {
        crate::intel::submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_immediate_gradient_probe(
            row_index,
            x_base,
            c_re,
            c_im,
        )
    };
    let raw_radius = if !gradient.readback_ok && row_constant.readback_ok {
        crate::intel::submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_immediate_raw_radius_probe(
            row_index,
            x_base,
            c_re,
            c_im,
        )
    } else {
        gradient
    };
    let use_linear_gradient = linear_constant.readback_ok && gradient.readback_ok;
    let use_raw_radius = !use_linear_gradient && raw_radius.readback_ok;
    crate::log!(
        "mandelbrot-gpu-sidequest: math-proof gradient_ok={} raw_radius_ok={} gradient_finished={} raw_radius_finished={} address_path={} sample_after=0x{:08X} expected=0x{:08X} lane_dispatch_delta={} finish_marker=0x{:08X} next={}\n",
        gradient.readback_ok as u8,
        raw_radius.readback_ok as u8,
        gradient.finished as u8,
        raw_radius.finished as u8,
        if use_linear_gradient {
            "linear-groupid"
        } else if use_raw_radius {
            "immediate-raw-radius"
        } else {
            "immediate-constant"
        },
        if use_raw_radius {
            raw_radius.output_first_after
        } else {
            gradient.output_first_after
        },
        if use_raw_radius {
            raw_radius.sentinel
        } else {
            gradient.sentinel
        },
        gradient.dispatch_delta.saturating_add(raw_radius.dispatch_delta),
        if use_raw_radius {
            raw_radius.finish_marker
        } else {
            gradient.finish_marker
        },
        if use_linear_gradient || use_raw_radius {
            "sweep-active-path"
        } else {
            "sweep-constant-fallback"
        },
    );

    let sweep_x_groups = scanout_w
        .saturating_add(pixels_per_program - 1)
        / pixels_per_program;
    let sweep_y_blocks = scanout_h
        .saturating_add(MANDELBROT16_BOOT_VISIBLE_STAMP_ROWS - 1)
        / MANDELBROT16_BOOT_VISIBLE_STAMP_ROWS;
    let mut frame = 0u64;
    let mut sweep_cursor = 0u32;
    loop {
        let mut submitted = 0u32;
        let mut finished = 0u32;
        let mut dispatch_delta = 0u64;
        let mut last = boot;
        let mut block = 0u32;
        while block < MANDELBROT16_SWEEP_X_BLOCKS_PER_FRAME {
            let linear = sweep_cursor % sweep_x_groups.max(1);
            let y_block = (sweep_cursor / sweep_x_groups.max(1)) % sweep_y_blocks.max(1);
            let x = linear.saturating_mul(pixels_per_program);
            let y = y_block.saturating_mul(MANDELBROT16_BOOT_VISIBLE_STAMP_ROWS);
            let c_re = mandelbrot16_pixel_c_re_q12(x);
            let c_im = mandelbrot16_block_c_im_q12(y_block, sweep_y_blocks);
            let proof = if use_linear_gradient {
                crate::intel::submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_quiet_linear_band(
                    y,
                    x,
                    MANDELBROT16_SWEEP_ROW_GROUPS,
                    1,
                    c_re,
                    c_im,
                )
            } else if use_raw_radius {
                crate::intel::submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_quiet_immediate_raw_radius_rows(
                    y,
                    x,
                    MANDELBROT16_SWEEP_ROW_GROUPS,
                    c_re,
                    c_im,
                )
            } else {
                let color = mandelbrot16_visible_constant_color(frame, linear, y_block);
                crate::intel::submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_quiet_immediate_constant(
                    y,
                    x,
                    color,
                )
            };
            if proof.submitted {
                submitted = submitted.saturating_add(1);
            }
            if proof.finished {
                finished = finished.saturating_add(1);
            }
            dispatch_delta = dispatch_delta.saturating_add(proof.dispatch_delta as u64);
            last = proof;
            sweep_cursor = if sweep_x_groups == 0 || sweep_y_blocks == 0 {
                0
            } else {
                (sweep_cursor + 1) % sweep_x_groups.saturating_mul(sweep_y_blocks)
            };
            if !proof.finished {
                break;
            }
            block += 1;
        }

        if submitted != 0 {
            let _ = crate::intel::notify_gpgpu_primary_scanout_external_write(
                "gpgpu-primary-scanout-mandelbrot16",
                0,
                MANDELBROT_GPGPU_PRESENT_FLUSH_BYTES,
            );
            if !released_lumen {
                crate::r::readiness::set(crate::r::readiness::MANDELBROT_GPU_SIDEQUEST_READY);
                released_lumen = true;
            }
        }

        if frame < 4 || frame % 64 == 0 || submitted != finished {
            crate::log!(
                "mandelbrot-gpu-sidequest: frame={} mode={} submitted={} finished={} cursor={} grid={}x{} dispatch_delta={} last_reason={} last_gpu=0x{:X} last_after=0x{:08X} finish_marker=0x{:08X} lumen_released={} summary=simd16-only\n",
                frame,
                if use_linear_gradient {
                    "linear-gradient"
                } else if use_raw_radius {
                    "raw-radius"
                } else {
                    "constant"
                },
                submitted,
                finished,
                sweep_cursor,
                sweep_x_groups,
                sweep_y_blocks,
                dispatch_delta,
                last.reason,
                last.output_gpu,
                last.output_first_after,
                last.finish_marker,
                released_lumen as u8,
            );
        }

        frame = frame.wrapping_add(1);
        Timer::after(EmbassyDuration::from_millis(MANDELBROT_GPGPU_LOOP_MS)).await;
    }
}

async fn park(reason: &'static str) -> ! {
    let mut heartbeat = 0u64;
    loop {
        if heartbeat == 0 || heartbeat % 16 == 0 {
            crate::log!(
                "mandelbrot-gpu-sidequest: parked heartbeat={} reason={} active_artifact={} next=boot-on-intel-gpgpu-hardware\n",
                heartbeat,
                reason,
                trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_PROGRAM_NAME,
            );
        }
        heartbeat = heartbeat.wrapping_add(1);
        Timer::after(EmbassyDuration::from_millis(1_000)).await;
    }
}
