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
    include_bytes!("../../crates/trueos-shader/mandelbrot_fragment_1440p_parametric.spv");
pub(crate) const MANDELBROT_TARGET_WIDTH: u32 = 2560;
pub(crate) const MANDELBROT_TARGET_HEIGHT: u32 = 1440;
pub(crate) const MANDELBROT_PUSH_CONSTANT_BYTES: u16 = 24;
pub(crate) const MANDELBROT_GPGPU_LOOP_MS: u64 = 100;
pub(crate) const MANDELBROT_GPGPU_PREVIEW_PIXELS_PER_TICK: usize = 8192;

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
        "mandelbrot-gpu-sidequest: attempted name={} called=1 hot=0 artifact_stage={} source={} spirv={} spirv_bytes={} spirv_sig=0x{:016X} target={}x{} rgba_bytes=0x{:X} push_constants={} render_path={} fallback_present={} scanout={}x{} primary_gpu=0x{:X} shader_helper_ready={} upload=custom-intel-gpgpu-program action=hold-lumen-load next=visible-gpgpu-primary-framebuffer-mandelbrot8-pilot\n",
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

    let marker_proof = crate::intel::submit_gpgpu_primary_scanout_marker_probe();
    crate::log!(
        "mandelbrot-gpu-sidequest: primary-scanout-marker-preflight submitted={} finished={} readback_ok={} reason={} program_source={} target_gpu=0x{:X} sentinel=0x{:08X} before=0x{:08X} after=0x{:08X} hit_mask=0x{:016X} finish_marker=0x{:08X} action={} next={}\n",
        marker_proof.submitted as u8,
        marker_proof.finished as u8,
        marker_proof.readback_ok as u8,
        marker_proof.reason,
        marker_proof.program_name,
        marker_proof.output_gpu,
        marker_proof.sentinel,
        marker_proof.output_first_before,
        marker_proof.output_first_after,
        marker_proof.output_hits_lo64,
        marker_proof.finish_marker,
        if marker_proof.readback_ok {
            "continue-mandelbrot8-strip-loop"
        } else {
            "diagnose-scanout-gpgpu-store"
        },
        if marker_proof.readback_ok {
            "visible-strip-pilot"
        } else {
            "fix-primary-scanout-target"
        },
    );

    let mut frame: u64 = 0;
    let mut released_lumen = false;
    let mut preview_cursor = 0usize;
    loop {
        let (proof, next_cursor) = crate::intel::submit_gpgpu_primary_scanout_mandelbrot_preview(
            preview_cursor,
            MANDELBROT_GPGPU_PREVIEW_PIXELS_PER_TICK,
        );
        preview_cursor = next_cursor;
        if proof.readback_ok && !released_lumen {
            crate::r::readiness::set(crate::r::readiness::MANDELBROT_GPU_SIDEQUEST_READY);
            released_lumen = true;
        }
        crate::log!(
            "mandelbrot-gpu-sidequest: gpgpu-primary-framebuffer-mandelbrot8-loop frame={} submitted={} finished={} readback_ok={} reason={} program_source={} target_gpu=0x{:X} first_expected=0x{:08X} before=0x{:08X} after=0x{:08X} lane_hit_mask=0x{:016X} finish_marker=0x{:08X} preview_cursor={} pixels_per_tick={} lumen_released={} action={} next={} does_not_prove=fragment_render_path\n",
            frame,
            proof.submitted as u8,
            proof.finished as u8,
            proof.readback_ok as u8,
            proof.reason,
            proof.program_name,
            proof.output_gpu,
            proof.sentinel,
            proof.output_first_before,
            proof.output_first_after,
            proof.output_hits_lo64,
            proof.finish_marker,
            preview_cursor,
            MANDELBROT_GPGPU_PREVIEW_PIXELS_PER_TICK,
            released_lumen as u8,
            if proof.readback_ok {
                "continue-sidequest-loop"
            } else {
                "hold-lumen-load"
            },
            if proof.readback_ok {
                "continue-visible-gpgpu-pilot"
            } else {
                "fix-gpgpu-mandelbrot8-strip"
            },
        );
        frame = frame.wrapping_add(1);
        Timer::after(EmbassyDuration::from_millis(MANDELBROT_GPGPU_LOOP_MS)).await;
    }
}
