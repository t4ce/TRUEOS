#![allow(dead_code)]

use embassy_executor::{SpawnError, Spawner};
use embassy_time::{Duration as EmbassyDuration, Timer};
use trueos_gfx_core::{ShaderDesc, ShaderFormat, ShaderStage};

pub(crate) const MANDELBROT_GPU_SIDEQUEST_NAME: &str = "mandelbrot-gpu-sidequest";
pub(crate) const MANDELBROT_FRAGMENT_SOURCE_PATH: &str =
    "crates/trueos-shader/mandelbrot_fragment_1440p_parametric.frag";
pub(crate) const MANDELBROT_FRAGMENT_SPIRV_PATH: &str =
    "crates/trueos-shader/mandelbrot_fragment_1440p_parametric.spv";
pub(crate) const MANDELBROT_FRAGMENT_SPIRV_BYTES: &[u8] =
    include_bytes!("../../../crates/trueos-shader/mandelbrot_fragment_1440p_parametric.spv");
pub(crate) const MANDELBROT_TARGET_WIDTH: u32 = 2560;
pub(crate) const MANDELBROT_TARGET_HEIGHT: u32 = 1440;
pub(crate) const MANDELBROT_PUSH_CONSTANT_BYTES: u16 = 24;
pub(crate) const MANDELBROT_GPGPU_LOOP_MS: u64 = 16;
pub(crate) const MANDELBROT_GPGPU_PREVIEW_PIXELS_PER_TICK: usize = 8192;
pub(crate) const MANDELBROT_GPGPU_RGB_ZOOM_RECT_WIDTH: u64 = 1280;
pub(crate) const MANDELBROT_GPGPU_RGB_ZOOM_RECT_HEIGHT: u64 = 192;
pub(crate) const MANDELBROT_GPGPU_ANIM_BAND_HEIGHT: u64 = 8;
pub(crate) const MANDELBROT_GPGPU_ANIM_PHASE_ROWS_PER_FRAME: u64 =
    MANDELBROT_GPGPU_ANIM_BAND_HEIGHT;
pub(crate) const MANDELBROT_GPGPU_FULL_FRAME_COLOR_FLIP: bool = true;
pub(crate) const MANDELBROT_GPGPU_GROUPID_LINE1280_ROWS_PER_BURST: u64 = 192;
// One full-height submit dispatches partway but misses the post-walker marker.
// Keep this baseline to one centered half-height area while we raise row-pilot parallelism.
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
pub(crate) const MANDELBROT_GPGPU_RUN_ROW2560_SIMD8_PROBE: bool = false;
pub(crate) const MANDELBROT_GPGPU_NOTIFY_AFTER_FULLSCREEN_SWEEP: bool = true;
pub(crate) const MANDELBROT_GPGPU_RUN_GROUPID_LINE320_PROBE: bool = false;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum MandelbrotGpuArtifactStage {
    Fragment,
    Compute,
}

impl MandelbrotGpuArtifactStage {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Fragment => "fragment",
            Self::Compute => "compute",
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum MandelbrotPresentPath {
    BufferFirst,
    CopyToIntelOverlay,
    DirectFramebuffer,
}

impl MandelbrotPresentPath {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::BufferFirst => "buffer-first",
            Self::CopyToIntelOverlay => "copy-to-intel-overlay",
            Self::DirectFramebuffer => "direct-framebuffer",
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct MandelbrotGpuSidequestPlan {
    pub(crate) name: &'static str,
    pub(crate) stage: MandelbrotGpuArtifactStage,
    pub(crate) source_path: &'static str,
    pub(crate) spirv_path: &'static str,
    pub(crate) target_width: u32,
    pub(crate) target_height: u32,
    pub(crate) push_constant_bytes: u16,
    pub(crate) render_path: MandelbrotPresentPath,
    pub(crate) fallback_present_path: MandelbrotPresentPath,
}

impl MandelbrotGpuSidequestPlan {
    pub(crate) const fn default_fragment_buffer_first() -> Self {
        Self {
            name: MANDELBROT_GPU_SIDEQUEST_NAME,
            stage: MandelbrotGpuArtifactStage::Compute,
            source_path: MANDELBROT_FRAGMENT_SOURCE_PATH,
            spirv_path: MANDELBROT_FRAGMENT_SPIRV_PATH,
            target_width: MANDELBROT_TARGET_WIDTH,
            target_height: MANDELBROT_TARGET_HEIGHT,
            push_constant_bytes: MANDELBROT_PUSH_CONSTANT_BYTES,
            render_path: MandelbrotPresentPath::DirectFramebuffer,
            fallback_present_path: MandelbrotPresentPath::CopyToIntelOverlay,
        }
    }

    pub(crate) const fn target_rgba_bytes(self) -> usize {
        (self.target_width as usize)
            .saturating_mul(self.target_height as usize)
            .saturating_mul(4)
    }
}

pub(crate) const fn mandelbrot_gpu_sidequest_plan() -> MandelbrotGpuSidequestPlan {
    MandelbrotGpuSidequestPlan::default_fragment_buffer_first()
}

pub(crate) fn mandelbrot_fragment_shader_desc(spirv_bytes: &'static [u8]) -> ShaderDesc<'static> {
    ShaderDesc {
        stage: ShaderStage::Fragment,
        format: ShaderFormat::SpirV,
        bytes: spirv_bytes,
    }
}

fn byte_signature(bytes: &[u8]) -> u64 {
    let mut sig = 0xCBF2_9CE4_8422_2325u64;
    for &byte in bytes {
        sig ^= byte as u64;
        sig = sig.wrapping_mul(0x0000_0100_0000_01B3);
    }
    sig
}

pub(crate) fn spawn_mandelbrot_gpu_sidequest(spawner: Spawner) -> Result<(), SpawnError> {
    let token = mandelbrot_gpu_sidequest_task()?;
    spawner.spawn(token);
    Ok(())
}

#[embassy_executor::task(pool_size = 1)]
pub(crate) async fn mandelbrot_gpu_sidequest_task() {
    let plan = mandelbrot_gpu_sidequest_plan();
    let scanout = crate::intel::active_scanout_dimensions();
    let scanout_w = scanout.map(|dims| dims.0).unwrap_or(0);
    let scanout_h = scanout.map(|dims| dims.1).unwrap_or(0);
    let primary_gpu = crate::intel::primary_surface_gpu_addr().unwrap_or(0);
    let shader_helper_ready = true;
    let shader_desc = mandelbrot_fragment_shader_desc(MANDELBROT_FRAGMENT_SPIRV_BYTES);
    let spirv_bytes = shader_desc.bytes.len();
    let spirv_sig = byte_signature(shader_desc.bytes);

    crate::log!(
        "mandelbrot-gpu-sidequest: attempted name={} called=1 hot=1 artifact_stage={} source={} spirv={} spirv_bytes={} spirv_sig=0x{:016X} target={}x{} rgba_bytes=0x{:X} push_constants={} render_path={} fallback_present={} scanout={}x{} primary_gpu=0x{:X} shader_helper_ready={} upload=custom-intel-gpgpu-program action=render-visible-groupid-line1280-row-frame next=raise-row-burst-or-fold-x-segment\n",
        plan.name,
        plan.stage.as_str(),
        plan.source_path,
        plan.spirv_path,
        spirv_bytes,
        spirv_sig,
        plan.target_width,
        plan.target_height,
        plan.target_rgba_bytes(),
        plan.push_constant_bytes,
        plan.render_path.as_str(),
        plan.fallback_present_path.as_str(),
        scanout_w,
        scanout_h,
        primary_gpu,
        shader_helper_ready as u8
    );

    let mut frame: u64 = 0;
    let mut released_lumen = false;
    if MANDELBROT_GPGPU_RUN_ROW2560_SIMD8_PROBE {
        let row_probe =
            crate::intel::submit_gpgpu_primary_scanout_row2560_simd8_probe(1, frame as u32);
        if row_probe.submitted && !released_lumen {
            crate::r::readiness::set(crate::r::readiness::MANDELBROT_GPU_SIDEQUEST_READY);
            released_lumen = true;
        }
        crate::log!(
            "mandelbrot-gpu-sidequest: gpgpu-primary-framebuffer-row2560-simd8-probe submitted={} finished={} readback_ok={} reason={} program_source={} target_gpu=0x{:X} sample_change_mask=0x{:016X} lane_dispatch_delta={} finish_marker=0x{:08X} lumen_released={} action={} next={} deliverable=one-submit-full-width-row\n",
            row_probe.submitted as u8,
            row_probe.finished as u8,
            row_probe.readback_ok as u8,
            row_probe.reason,
            row_probe.program_name,
            row_probe.output_gpu,
            row_probe.output_hits_lo64,
            row_probe.dispatch_delta,
            row_probe.finish_marker,
            released_lumen as u8,
            if row_probe.readback_ok {
                "continue-fullscreen-line-pilot"
            } else {
                "keep-line1280-sweep-while-fixing-row2560"
            },
            if row_probe.readback_ok {
                "promote-row2560-simd8-sweep"
            } else {
                "fix-simd8-store-payload"
            },
        );
    }
    if MANDELBROT_GPGPU_RUN_GROUPID_LINE320_PROBE {
        let groupid_probe = crate::intel::submit_gpgpu_primary_scanout_groupid_line320_probe(1, 2);
        if groupid_probe.submitted && !released_lumen {
            crate::r::readiness::set(crate::r::readiness::MANDELBROT_GPU_SIDEQUEST_READY);
            released_lumen = true;
        }
        crate::log!(
            "mandelbrot-gpu-sidequest: gpgpu-primary-framebuffer-groupid-line320-probe submitted={} finished={} readback_ok={} reason={} program_source={} target_gpu=0x{:X} group_hit_mask=0x{:02X} lane_dispatch_delta={} finish_marker=0x{:08X} lumen_released={} action={} next={} deliverable=one-submit-pilot-id-visible-blocks\n",
            groupid_probe.submitted as u8,
            groupid_probe.finished as u8,
            groupid_probe.readback_ok as u8,
            groupid_probe.reason,
            groupid_probe.program_name,
            groupid_probe.output_gpu,
            groupid_probe.output_hits_lo64 as u32,
            groupid_probe.dispatch_delta,
            groupid_probe.finish_marker,
            released_lumen as u8,
            if groupid_probe.readback_ok {
                "continue-line1280-baseline-with-pilot-id-proven"
            } else {
                "continue-line1280-baseline-without-pilot-id"
            },
            if groupid_probe.readback_ok {
                "promote-groupid-row2560"
            } else {
                "fix-groupid-payload"
            },
        );
    }
    let line_pixels = trueos_eu::gfx12::PRIMARY_SCANOUT_LINE1280_SCALAR_BW_LANES as u64;
    let rect_w = core::cmp::min(
        core::cmp::min(MANDELBROT_GPGPU_RGB_ZOOM_RECT_WIDTH, scanout_w as u64),
        line_pixels,
    );
    let rect_h = core::cmp::min(MANDELBROT_GPGPU_RGB_ZOOM_RECT_HEIGHT, scanout_h as u64);
    let rect_x = (scanout_w as u64).saturating_sub(rect_w) / 2;
    let rect_y = (scanout_h as u64).saturating_sub(rect_h) / 2;
    let rows_per_segment = 1u64;
    let segments_per_row =
        core::cmp::max(1, rect_w.saturating_add(line_pixels.saturating_sub(1)) / line_pixels);
    let row_groups_per_frame = core::cmp::max(
        1,
        rect_h.saturating_add(rows_per_segment.saturating_sub(1)) / rows_per_segment,
    );
    let submits_per_tick = row_groups_per_frame;
    let row_groups_per_burst = if MANDELBROT_GPGPU_FULL_FRAME_COLOR_FLIP {
        core::cmp::min(row_groups_per_frame, MANDELBROT_GPGPU_GROUPID_LINE1280_ROWS_PER_BURST)
    } else {
        core::cmp::max(
            1,
            MANDELBROT_GPGPU_ANIM_BAND_HEIGHT.saturating_add(rows_per_segment.saturating_sub(1))
                / rows_per_segment,
        )
    };
    let bursts_per_frame = row_groups_per_frame
        .saturating_add(row_groups_per_burst.saturating_sub(1))
        / row_groups_per_burst;
    loop {
        let first_serial = frame.saturating_mul(submits_per_tick);
        let mut submitted = 0u64;
        let mut finished = 0u64;
        let mut readback_ok = 0u64;
        let mut dispatch_delta = 0u64;
        let mut burst = 0u64;
        let mut last_proof = None;
        while burst < bursts_per_frame {
            let local_row_group = burst.saturating_mul(row_groups_per_burst);
            let row_groups_this_burst = core::cmp::min(
                row_groups_per_burst,
                row_groups_per_frame.saturating_sub(local_row_group),
            );
            let local_row = local_row_group.saturating_mul(rows_per_segment);
            let phase_rows = frame.wrapping_mul(MANDELBROT_GPGPU_ANIM_PHASE_ROWS_PER_FRAME);
            let band = local_row.saturating_add(phase_rows) / MANDELBROT_GPGPU_ANIM_BAND_HEIGHT;
            let color_seed = if MANDELBROT_GPGPU_FULL_FRAME_COLOR_FLIP {
                MANDELBROT_GPGPU_ANIM_PALETTE[(frame & 7) as usize]
            } else {
                MANDELBROT_GPGPU_ANIM_PALETTE[((band.saturating_add(frame)) & 7) as usize]
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
            let proof_ok = proof.readback_ok;
            last_proof = Some(proof);
            if !proof_ok {
                break;
            }
            burst += 1;
        }
        let frame_notified = submitted != 0
            && readback_ok == submitted
            && MANDELBROT_GPGPU_NOTIFY_AFTER_FULLSCREEN_SWEEP
            && crate::intel::notify_gpgpu_primary_scanout_external_write(
                "gpgpu-primary-scanout-line1280-frame",
                0,
                MANDELBROT_GPGPU_PRESENT_FLUSH_BYTES,
            );

        let should_log_frame = frame < 4 || frame % 64 == 0 || readback_ok != submitted;
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
                bursts_per_frame,
                MANDELBROT_GPGPU_FULL_FRAME_COLOR_FLIP as u8,
                MANDELBROT_GPGPU_ANIM_BAND_HEIGHT,
                frame.wrapping_mul(MANDELBROT_GPGPU_ANIM_PHASE_ROWS_PER_FRAME),
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
