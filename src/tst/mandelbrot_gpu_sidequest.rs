#![allow(dead_code)]

use embassy_executor::{SpawnError, Spawner};
use trueos_gfx_core::{ShaderDesc, ShaderFormat, ShaderStage};

pub(crate) const MANDELBROT_GPU_SIDEQUEST_NAME: &str = "mandelbrot-gpu-sidequest";
pub(crate) const MANDELBROT_FRAGMENT_SOURCE_PATH: &str =
    ".codex_tmp/mandelbrot_fragment_1440p_parametric.frag";
pub(crate) const MANDELBROT_FRAGMENT_SPIRV_PATH: &str =
    ".codex_tmp/mandelbrot_fragment_1440p_parametric.spv";
pub(crate) const MANDELBROT_TARGET_WIDTH: u32 = 2560;
pub(crate) const MANDELBROT_TARGET_HEIGHT: u32 = 1440;
pub(crate) const MANDELBROT_PUSH_CONSTANT_BYTES: u16 = 24;

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
            stage: MandelbrotGpuArtifactStage::Fragment,
            source_path: MANDELBROT_FRAGMENT_SOURCE_PATH,
            spirv_path: MANDELBROT_FRAGMENT_SPIRV_PATH,
            target_width: MANDELBROT_TARGET_WIDTH,
            target_height: MANDELBROT_TARGET_HEIGHT,
            push_constant_bytes: MANDELBROT_PUSH_CONSTANT_BYTES,
            render_path: MandelbrotPresentPath::BufferFirst,
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

    crate::log!(
        "mandelbrot-gpu-sidequest: scaffold name={} called=1 hot=0 artifact_stage={} source={} spirv={} target={}x{} rgba_bytes=0x{:X} push_constants={} render_path={} fallback_present={} scanout={}x{} upload=deferred action=no-submit-no-present\n",
        plan.name,
        plan.stage.as_str(),
        plan.source_path,
        plan.spirv_path,
        plan.target_width,
        plan.target_height,
        plan.target_rgba_bytes(),
        plan.push_constant_bytes,
        plan.render_path.as_str(),
        plan.fallback_present_path.as_str(),
        scanout_w,
        scanout_h
    );
}
