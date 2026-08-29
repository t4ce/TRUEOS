pub(crate) fn copy_rect_rgba8_upload_status() -> Option<UploadedKernelArtifact> {
    *COPY_RECT_RGBA8_UPLOAD.lock()
}

pub(crate) fn subset_sum_collapse5_merge10_upload_status() -> Option<UploadedKernelArtifact> {
    *SUBSET_SUM_COLLAPSE5_MERGE10_UPLOAD.lock()
}

pub(crate) fn fill_rect_rgba8_upload_status() -> Option<UploadedKernelArtifact> {
    *FILL_RECT_RGBA8_UPLOAD.lock()
}

pub(crate) fn fill_rect_worklist_rgba8_upload_status() -> Option<UploadedKernelArtifact> {
    *FILL_RECT_WORKLIST_RGBA8_UPLOAD.lock()
}

pub(crate) fn gradient_rect_worklist_rgba8_upload_status() -> Option<UploadedKernelArtifact> {
    *GRADIENT_RECT_WORKLIST_RGBA8_UPLOAD.lock()
}

pub(crate) fn alpha_blend_worklist_rgba8_upload_status() -> Option<UploadedKernelArtifact> {
    *ALPHA_BLEND_WORKLIST_RGBA8_UPLOAD.lock()
}

pub(crate) fn glyph_mask_rgba8_upload_status() -> Option<UploadedKernelArtifact> {
    *GLYPH_MASK_RGBA8_UPLOAD.lock()
}

pub(crate) fn sprite_quad_worklist_rgba8_upload_status() -> Option<UploadedKernelArtifact> {
    *SPRITE_QUAD_WORKLIST_RGBA8_UPLOAD.lock()
}

pub(crate) fn ui4_compose_layers_rgba8_upload_status() -> Option<UploadedKernelArtifact> {
    *UI4_COMPOSE_LAYERS_RGBA8_UPLOAD.lock()
}

pub(crate) fn mandel64_worklist_rgba8_upload_status() -> Option<UploadedKernelArtifact> {
    *MANDEL64_WORKLIST_RGBA8_UPLOAD.lock()
}

pub(crate) fn skybox_sample_rgb565_upload_status() -> Option<UploadedKernelArtifact> {
    *SKYBOX_SAMPLE_RGB565_UPLOAD.lock()
}

pub(crate) fn chart_sine_rgba8_upload_status() -> Option<UploadedKernelArtifact> {
    *CHART_SINE_RGBA8_UPLOAD.lock()
}

pub(crate) fn pixel_plasma_rgba8_upload_status() -> Option<UploadedKernelArtifact> {
    *PIXEL_PLASMA_RGBA8_UPLOAD.lock()
}

pub(crate) fn cpp_demo_rgba8_upload_status() -> Option<UploadedKernelArtifact> {
    *CPP_DEMO_RGBA8_UPLOAD.lock()
}

pub(crate) fn cpp_audio_visualizer_rgba8_upload_status() -> Option<UploadedKernelArtifact> {
    *CPP_AUDIO_VISUALIZER_RGBA8_UPLOAD.lock()
}

pub(crate) fn particle_craft_upload_status() -> Option<UploadedKernelArtifact> {
    *PARTICLE_CRAFT_UPLOAD.lock()
}

pub(crate) fn font_instance_rgba8_upload_status() -> Option<UploadedKernelArtifact> {
    *FONT_INSTANCE_RGBA8_UPLOAD.lock()
}

pub(crate) fn lfm25_q8_project_packed_upload_status() -> Option<UploadedKernelArtifact> {
    *LFM25_Q8_PROJECT_PACKED_UPLOAD.lock()
}

pub(crate) fn kokoro_qgemm_u8_i8_upload_status() -> Option<UploadedKernelArtifact> {
    *KOKORO_QGEMM_U8_I8_UPLOAD.lock()
}

pub(crate) fn kokoro_conv1d_u8_u8_upload_status() -> Option<UploadedKernelArtifact> {
    *KOKORO_CONV1D_U8_U8_UPLOAD.lock()
}

pub(crate) fn spirit_vfx_background_rgba8_upload_status() -> Option<UploadedKernelArtifact> {
    *SPIRIT_VFX_BACKGROUND_RGBA8_UPLOAD.lock()
}

pub(crate) fn spirit_vfx_sprite_rgba8_upload_status() -> Option<UploadedKernelArtifact> {
    *SPIRIT_VFX_SPRITE_RGBA8_UPLOAD.lock()
}

pub(crate) fn font_outline_coverage_r8_upload_status() -> Option<UploadedKernelArtifact> {
    *FONT_OUTLINE_COVERAGE_R8_UPLOAD.lock()
}

pub(crate) fn upload_copy_rect_rgba8_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *COPY_RECT_RGBA8_UPLOAD.lock() {
        return Some(upload);
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: copy-rect-rgba8 upload skipped reason=no-claimed-device\n"
        );
        return None;
    };

    let upload = upload_artifact(dev, COPY_RECT_RGBA8_ADLS_ARTIFACT, COPY_RECT_RGBA8_ADLS_GPU)?;
    *COPY_RECT_RGBA8_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_subset_sum_collapse5_merge10_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *SUBSET_SUM_COLLAPSE5_MERGE10_UPLOAD.lock() {
        return Some(upload);
    }
    let dev = super::claimed_device()?;
    let upload = upload_artifact(
        dev,
        SUBSET_SUM_COLLAPSE5_MERGE10_ADLS_ARTIFACT,
        SUBSET_SUM_COLLAPSE5_MERGE10_ADLS_GPU,
    )?;
    *SUBSET_SUM_COLLAPSE5_MERGE10_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_fill_rect_rgba8_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *FILL_RECT_RGBA8_UPLOAD.lock() {
        return Some(upload);
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: fill-rect-rgba8 upload skipped reason=no-claimed-device\n"
        );
        return None;
    };

    let upload = upload_artifact(dev, FILL_RECT_RGBA8_ADLS_ARTIFACT, FILL_RECT_RGBA8_ADLS_GPU)?;
    *FILL_RECT_RGBA8_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_fill_rect_worklist_rgba8_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *FILL_RECT_WORKLIST_RGBA8_UPLOAD.lock() {
        return Some(upload);
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: fill-rect-worklist-rgba8 upload skipped reason=no-claimed-device\n"
        );
        return None;
    };

    let upload = upload_artifact(
        dev,
        FILL_RECT_WORKLIST_RGBA8_ADLS_ARTIFACT,
        FILL_RECT_WORKLIST_RGBA8_ADLS_GPU,
    )?;
    *FILL_RECT_WORKLIST_RGBA8_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_gradient_rect_worklist_rgba8_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *GRADIENT_RECT_WORKLIST_RGBA8_UPLOAD.lock() {
        return Some(upload);
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: gradient-rect-worklist-rgba8 upload skipped reason=no-claimed-device\n"
        );
        return None;
    };

    let upload = upload_artifact(
        dev,
        GRADIENT_RECT_WORKLIST_RGBA8_ADLS_ARTIFACT,
        GRADIENT_RECT_WORKLIST_RGBA8_ADLS_GPU,
    )?;
    *GRADIENT_RECT_WORKLIST_RGBA8_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_alpha_blend_worklist_rgba8_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *ALPHA_BLEND_WORKLIST_RGBA8_UPLOAD.lock() {
        return Some(upload);
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: alpha-blend-worklist-rgba8 upload skipped reason=no-claimed-device\n"
        );
        return None;
    };

    let upload = upload_artifact(
        dev,
        ALPHA_BLEND_WORKLIST_RGBA8_ADLS_ARTIFACT,
        ALPHA_BLEND_WORKLIST_RGBA8_ADLS_GPU,
    )?;
    *ALPHA_BLEND_WORKLIST_RGBA8_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_glyph_mask_rgba8_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *GLYPH_MASK_RGBA8_UPLOAD.lock() {
        return Some(upload);
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: glyph-mask-rgba8 upload skipped reason=no-claimed-device\n"
        );
        return None;
    };

    let upload = upload_artifact(dev, GLYPH_MASK_RGBA8_ADLS_ARTIFACT, GLYPH_MASK_RGBA8_ADLS_GPU)?;
    *GLYPH_MASK_RGBA8_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_font_instance_rgba8_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *FONT_INSTANCE_RGBA8_UPLOAD.lock() {
        return Some(upload);
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: font-instance-rgba8 upload skipped reason=no-claimed-device\n"
        );
        return None;
    };

    let upload =
        upload_artifact(dev, FONT_INSTANCE_RGBA8_ADLS_ARTIFACT, FONT_INSTANCE_RGBA8_ADLS_GPU)?;
    *FONT_INSTANCE_RGBA8_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_ui4_nv12_tile64_to_rgba8_frame_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *UI4_NV12_TILE64_TO_RGBA8_FRAME_UPLOAD.lock() {
        return Some(upload);
    }
    let dev = super::claimed_device()?;
    let upload = upload_artifact(
        dev,
        UI4_NV12_TILE64_TO_RGBA8_FRAME_ADLS_ARTIFACT,
        UI4_NV12_TILE64_TO_RGBA8_FRAME_ADLS_GPU,
    )?;
    *UI4_NV12_TILE64_TO_RGBA8_FRAME_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_sprite_quad_worklist_rgba8_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *SPRITE_QUAD_WORKLIST_RGBA8_UPLOAD.lock() {
        return Some(upload);
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: sprite-quad-worklist-rgba8 upload skipped reason=no-claimed-device\n"
        );
        return None;
    };

    let upload = upload_artifact(
        dev,
        SPRITE_QUAD_WORKLIST_RGBA8_ADLS_ARTIFACT,
        SPRITE_QUAD_WORKLIST_RGBA8_ADLS_GPU,
    )?;
    *SPRITE_QUAD_WORKLIST_RGBA8_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_ui4_compose_layers_rgba8_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *UI4_COMPOSE_LAYERS_RGBA8_UPLOAD.lock() {
        return Some(upload);
    }

    let dev = super::claimed_device()?;
    let upload = upload_artifact(
        dev,
        UI4_COMPOSE_LAYERS_RGBA8_ADLS_ARTIFACT,
        UI4_COMPOSE_LAYERS_RGBA8_ADLS_GPU,
    )?;
    *UI4_COMPOSE_LAYERS_RGBA8_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_mandel64_worklist_rgba8_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *MANDEL64_WORKLIST_RGBA8_UPLOAD.lock() {
        return Some(upload);
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: mandel64-worklist-rgba8 upload skipped reason=no-claimed-device\n"
        );
        return None;
    };

    let upload = upload_artifact(
        dev,
        MANDEL64_WORKLIST_RGBA8_ADLS_ARTIFACT,
        MANDEL64_WORKLIST_RGBA8_ADLS_GPU,
    )?;
    *MANDEL64_WORKLIST_RGBA8_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_skybox_sample_rgb565_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *SKYBOX_SAMPLE_RGB565_UPLOAD.lock() {
        return Some(upload);
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: skybox-sample-rgb565 upload skipped reason=no-claimed-device\n"
        );
        return None;
    };

    let upload =
        upload_artifact(dev, SKYBOX_SAMPLE_RGB565_ADLS_ARTIFACT, SKYBOX_SAMPLE_RGB565_ADLS_GPU)?;
    *SKYBOX_SAMPLE_RGB565_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_chart_sine_rgba8_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *CHART_SINE_RGBA8_UPLOAD.lock() {
        return Some(upload);
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_warn!(
            target: "gpgpu";
            "intel/gpgpu: chart-sine-rgba8 upload skipped reason=no-claimed-device\n"
        );
        return None;
    };

    let upload = upload_artifact(dev, CHART_SINE_RGBA8_ADLS_ARTIFACT, CHART_SINE_RGBA8_ADLS_GPU)?;
    *CHART_SINE_RGBA8_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_pixel_plasma_rgba8_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *PIXEL_PLASMA_RGBA8_UPLOAD.lock() {
        return Some(upload);
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_warn!(
            target: "gpgpu";
            "intel/gpgpu: pixel-plasma-rgba8 upload skipped reason=no-claimed-device\n"
        );
        return None;
    };

    let upload =
        upload_artifact(dev, PIXEL_PLASMA_RGBA8_ADLS_ARTIFACT, PIXEL_PLASMA_RGBA8_ADLS_GPU)?;
    *PIXEL_PLASMA_RGBA8_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_cpp_demo_rgba8_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *CPP_DEMO_RGBA8_UPLOAD.lock() {
        return Some(upload);
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_warn!(
            target: "gpgpu";
            "intel/gpgpu: cpp-demo-rgba8 upload skipped reason=no-claimed-device\n"
        );
        return None;
    };

    let upload = upload_artifact(dev, CPP_DEMO_RGBA8_ADLS_ARTIFACT, CPP_DEMO_RGBA8_ADLS_GPU)?;
    *CPP_DEMO_RGBA8_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_shadertoy_kernel(shader_id: u32) -> Option<UploadedKernelArtifact> {
    let (artifact, gpu, slot) = match shader_id {
        SHADERTOY_SHADER_MANDELBROT => (
            SHADERTOY_MANDELBROT_ADLS_ARTIFACT,
            SHADERTOY_MANDELBROT_ADLS_GPU,
            &SHADERTOY_MANDELBROT_UPLOAD,
        ),
        SHADERTOY_SHADER_CUBE_FIELD => (
            SHADERTOY_CUBE_FIELD_ADLS_ARTIFACT,
            SHADERTOY_CUBE_FIELD_ADLS_GPU,
            &SHADERTOY_CUBE_FIELD_UPLOAD,
        ),
        SHADERTOY_SHADER_NGUYEN => {
            (SHADERTOY_NGUYEN_ADLS_ARTIFACT, SHADERTOY_NGUYEN_ADLS_GPU, &SHADERTOY_NGUYEN_UPLOAD)
        }
        _ => return None,
    };
    if let Some(upload) = *slot.lock() {
        return Some(upload);
    }
    let dev = super::claimed_device()?;
    let upload = upload_ppgtt_resident_artifact(dev, artifact, gpu)?;
    *slot.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_cpp_audio_visualizer_rgba8_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *CPP_AUDIO_VISUALIZER_RGBA8_UPLOAD.lock() {
        return Some(upload);
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_warn!(
            target: "gpgpu";
            "intel/gpgpu: cpp-audio-visualizer-rgba8 upload skipped reason=no-claimed-device\n"
        );
        return None;
    };

    let upload = upload_artifact(
        dev,
        CPP_AUDIO_VISUALIZER_RGBA8_ADLS_ARTIFACT,
        CPP_AUDIO_VISUALIZER_RGBA8_ADLS_GPU,
    )?;
    *CPP_AUDIO_VISUALIZER_RGBA8_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_particle_craft_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *PARTICLE_CRAFT_UPLOAD.lock() {
        return Some(upload);
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_warn!(
            target: "gpgpu";
            "intel/gpgpu: particle-craft upload skipped reason=no-claimed-device\n"
        );
        return None;
    };

    let upload = upload_artifact(dev, PARTICLE_CRAFT_ADLS_ARTIFACT, PARTICLE_CRAFT_ADLS_GPU)?;
    *PARTICLE_CRAFT_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_lfm25_q8_project_packed_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *LFM25_Q8_PROJECT_PACKED_UPLOAD.lock() {
        return Some(upload);
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_warn!(
            target: "gpgpu";
            "intel/gpgpu: lfm25-q8-project-packed upload skipped reason=no-claimed-device\n"
        );
        return None;
    };

    let upload = upload_artifact(
        dev,
        LFM25_Q8_PROJECT_PACKED_ADLS_ARTIFACT,
        LFM25_Q8_PROJECT_PACKED_ADLS_GPU,
    )?;
    *LFM25_Q8_PROJECT_PACKED_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_kokoro_qgemm_u8_i8_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *KOKORO_QGEMM_U8_I8_UPLOAD.lock() {
        return Some(upload);
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_warn!(
            target: "gpgpu";
            "intel/gpgpu: kokoro-qgemm-u8-i8 upload skipped reason=no-claimed-device\n"
        );
        return None;
    };

    let upload =
        upload_artifact(dev, KOKORO_QGEMM_U8_I8_ADLS_ARTIFACT, KOKORO_QGEMM_U8_I8_ADLS_GPU)?;
    *KOKORO_QGEMM_U8_I8_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_kokoro_conv1d_u8_u8_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *KOKORO_CONV1D_U8_U8_UPLOAD.lock() {
        return Some(upload);
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_warn!(
            target: "gpgpu";
            "intel/gpgpu: kokoro-conv1d-u8-u8 upload skipped reason=no-claimed-device\n"
        );
        return None;
    };

    let upload =
        upload_artifact(dev, KOKORO_CONV1D_U8_U8_ADLS_ARTIFACT, KOKORO_CONV1D_U8_U8_ADLS_GPU)?;
    *KOKORO_CONV1D_U8_U8_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_font_outline_coverage_r8_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *FONT_OUTLINE_COVERAGE_R8_UPLOAD.lock() {
        return Some(upload);
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_warn!(
            target: "gpgpu";
            "intel/gpgpu: font-outline-coverage-r8 upload skipped reason=no-claimed-device\n"
        );
        return None;
    };

    let upload = upload_artifact(
        dev,
        FONT_OUTLINE_COVERAGE_R8_ADLS_ARTIFACT,
        FONT_OUTLINE_COVERAGE_R8_ADLS_GPU,
    )?;
    *FONT_OUTLINE_COVERAGE_R8_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_helio_retained_transform_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *HELIO_RETAINED_TRANSFORM_UPLOAD.lock() {
        return Some(upload);
    }
    let dev = super::claimed_device()?;
    let upload = upload_ppgtt_resident_artifact(
        dev,
        HELIO_RETAINED_TRANSFORM_ADLS_ARTIFACT,
        HELIO_RETAINED_TRANSFORM_ADLS_GPU,
    )?;
    *HELIO_RETAINED_TRANSFORM_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_lab256_multiphase_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *LAB256_MULTIPHASE_UPLOAD.lock() {
        return Some(upload);
    }
    let dev = super::claimed_device()?;
    let upload = upload_ppgtt_resident_artifact(
        dev,
        LAB256_MULTIPHASE_ADLS_ARTIFACT,
        LAB256_MULTIPHASE_ADLS_GPU,
    )?;
    *LAB256_MULTIPHASE_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_spirit_vfx_background_rgba8_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *SPIRIT_VFX_BACKGROUND_RGBA8_UPLOAD.lock() {
        return Some(upload);
    }
    let dev = super::claimed_device()?;
    let upload = upload_ppgtt_resident_artifact(
        dev,
        SPIRIT_VFX_BACKGROUND_RGBA8_ADLS_ARTIFACT,
        SPIRIT_VFX_BACKGROUND_RGBA8_ADLS_GPU,
    )?;
    *SPIRIT_VFX_BACKGROUND_RGBA8_UPLOAD.lock() = Some(upload);
    Some(upload)
}

pub(crate) fn upload_spirit_vfx_sprite_rgba8_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *SPIRIT_VFX_SPRITE_RGBA8_UPLOAD.lock() {
        return Some(upload);
    }
    let dev = super::claimed_device()?;
    let upload = upload_ppgtt_resident_artifact(
        dev,
        SPIRIT_VFX_SPRITE_RGBA8_ADLS_ARTIFACT,
        SPIRIT_VFX_SPRITE_RGBA8_ADLS_GPU,
    )?;
    *SPIRIT_VFX_SPRITE_RGBA8_UPLOAD.lock() = Some(upload);
    Some(upload)
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum GpgpuArtifactReloadError {
    UnknownKernel,
    NoClaimedDevice,
    UploadFailed,
}

impl GpgpuArtifactReloadError {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::UnknownKernel => "unknown-kernel",
            Self::NoClaimedDevice => "no-claimed-device",
            Self::UploadFailed => "upload-failed",
        }
    }
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct GpgpuArtifactReloadSummary {
    pub(crate) attempted: usize,
    pub(crate) reloaded: usize,
    pub(crate) failed: usize,
}

struct GpgpuKnownArtifactSlot {
    artifact: GpgpuKernelArtifact,
    gpu: u64,
    upload: &'static Mutex<Option<UploadedKernelArtifact>>,
}

const GPGPU_KNOWN_ARTIFACT_NAMES: &[&str] = &[
    COPY_RECT_RGBA8_KERNEL_NAME,
    FILL_RECT_RGBA8_KERNEL_NAME,
    FILL_RECT_WORKLIST_RGBA8_KERNEL_NAME,
    GRADIENT_RECT_WORKLIST_RGBA8_KERNEL_NAME,
    ALPHA_BLEND_WORKLIST_RGBA8_KERNEL_NAME,
    GLYPH_MASK_RGBA8_KERNEL_NAME,
    SPRITE_QUAD_WORKLIST_RGBA8_KERNEL_NAME,
    UI4_COMPOSE_LAYERS_RGBA8_KERNEL_NAME,
    MANDEL64_WORKLIST_RGBA8_KERNEL_NAME,
    SKYBOX_SAMPLE_RGB565_KERNEL_NAME,
    CHART_SINE_RGBA8_KERNEL_NAME,
    PIXEL_PLASMA_RGBA8_KERNEL_NAME,
    CPP_DEMO_RGBA8_KERNEL_NAME,
    SHADERTOY_MANDELBROT_KERNEL_NAME,
    SHADERTOY_CUBE_FIELD_KERNEL_NAME,
    SHADERTOY_NGUYEN_KERNEL_NAME,
    PARTICLE_CRAFT_KERNEL_NAME,
    FONT_INSTANCE_RGBA8_KERNEL_NAME,
    LFM25_Q8_PROJECT_PACKED_KERNEL_NAME,
    KOKORO_QGEMM_U8_I8_KERNEL_NAME,
    KOKORO_CONV1D_U8_U8_KERNEL_NAME,
    SUBSET_SUM_COLLAPSE5_MERGE10_KERNEL_NAME,
    FONT_OUTLINE_COVERAGE_R8_KERNEL_NAME,
    HELIO_RETAINED_TRANSFORM_KERNEL_NAME,
    LAB256_MULTIPHASE_KERNEL_NAME,
    SPIRIT_VFX_BACKGROUND_RGBA8_KERNEL_NAME,
    SPIRIT_VFX_SPRITE_RGBA8_KERNEL_NAME,
];

pub(crate) fn reload_known_kernel_artifact(
    name: &str,
) -> Result<UploadedKernelArtifact, GpgpuArtifactReloadError> {
    // A first reload may need one allocation; later exact-byte reloads reuse
    // it. Serialize explicit reload callers so two first reloads cannot both
    // observe an empty slot and allocate the same GPU VA concurrently.
    static RELOAD_LOCK: Mutex<()> = Mutex::new(());
    let _reload_guard = RELOAD_LOCK.lock();
    let Some(slot) = known_artifact_slot(name) else {
        return Err(GpgpuArtifactReloadError::UnknownKernel);
    };
    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: {} reload failed reason=no-claimed-device\n",
            slot.artifact.name
        );
        return Err(GpgpuArtifactReloadError::NoClaimedDevice);
    };

    let reusable_upload = *slot.upload.lock();
    let address_space = known_artifact_address_space(slot.artifact.name);
    let Some(upload) = upload_artifact_from_sources(
        dev,
        slot.artifact,
        slot.gpu,
        address_space,
        true,
        reusable_upload,
    ) else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: {} reload failed reason=upload-failed previous=kept\n",
            slot.artifact.name
        );
        return Err(GpgpuArtifactReloadError::UploadFailed);
    };

    *slot.upload.lock() = Some(upload);
    crate::log_info!(
        target: "gpgpu";
        "intel/gpgpu: {} reload ok=1 source={} gpu=0x{:X} bytes=0x{:X} sha256={}\n",
        upload.name,
        upload.source,
        upload.gpu,
        upload.bytes,
        digest_hex(&upload.bin_sha256).as_str()
    );
    Ok(upload)
}

fn known_artifact_address_space(name: &str) -> GpgpuArtifactAddressSpace {
    match name {
        COPY_RECT_RGBA8_KERNEL_NAME
        | FILL_RECT_RGBA8_KERNEL_NAME
        | FILL_RECT_WORKLIST_RGBA8_KERNEL_NAME
        | ALPHA_BLEND_WORKLIST_RGBA8_KERNEL_NAME
        | GLYPH_MASK_RGBA8_KERNEL_NAME
        | FONT_INSTANCE_RGBA8_KERNEL_NAME
        | UI4_NV12_TILE64_TO_RGBA8_FRAME_KERNEL_NAME
        | SPRITE_QUAD_WORKLIST_RGBA8_KERNEL_NAME
        | UI4_COMPOSE_LAYERS_RGBA8_KERNEL_NAME
        | MANDEL64_WORKLIST_RGBA8_KERNEL_NAME
        | SKYBOX_SAMPLE_RGB565_KERNEL_NAME
        | CHART_SINE_RGBA8_KERNEL_NAME
        | PIXEL_PLASMA_RGBA8_KERNEL_NAME
        | CPP_DEMO_RGBA8_KERNEL_NAME
        | SHADERTOY_MANDELBROT_KERNEL_NAME
        | SHADERTOY_CUBE_FIELD_KERNEL_NAME
        | SHADERTOY_NGUYEN_KERNEL_NAME
        | CPP_AUDIO_VISUALIZER_RGBA8_KERNEL_NAME
        | PARTICLE_CRAFT_KERNEL_NAME
        | LFM25_Q8_PROJECT_PACKED_KERNEL_NAME
        | KOKORO_QGEMM_U8_I8_KERNEL_NAME
        | KOKORO_CONV1D_U8_U8_KERNEL_NAME
        | SUBSET_SUM_COLLAPSE5_MERGE10_KERNEL_NAME
        | FONT_OUTLINE_COVERAGE_R8_KERNEL_NAME
        | HELIO_RETAINED_TRANSFORM_KERNEL_NAME
        | LAB256_MULTIPHASE_KERNEL_NAME
        | SPIRIT_VFX_BACKGROUND_RGBA8_KERNEL_NAME
        | SPIRIT_VFX_SPRITE_RGBA8_KERNEL_NAME => GpgpuArtifactAddressSpace::CallerPpgtt,
        _ => GpgpuArtifactAddressSpace::GlobalGgtt,
    }
}

pub(crate) fn reload_all_known_kernel_artifacts() -> GpgpuArtifactReloadSummary {
    let mut summary = GpgpuArtifactReloadSummary::default();
    for name in GPGPU_KNOWN_ARTIFACT_NAMES {
        summary.attempted = summary.attempted.saturating_add(1);
        match reload_known_kernel_artifact(name) {
            Ok(_) => summary.reloaded = summary.reloaded.saturating_add(1),
            Err(_) => summary.failed = summary.failed.saturating_add(1),
        }
    }
    summary
}

fn known_artifact_slot(name: &str) -> Option<GpgpuKnownArtifactSlot> {
    match name {
        COPY_RECT_RGBA8_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: COPY_RECT_RGBA8_ADLS_ARTIFACT,
            gpu: COPY_RECT_RGBA8_ADLS_GPU,
            upload: &COPY_RECT_RGBA8_UPLOAD,
        }),
        FILL_RECT_RGBA8_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: FILL_RECT_RGBA8_ADLS_ARTIFACT,
            gpu: FILL_RECT_RGBA8_ADLS_GPU,
            upload: &FILL_RECT_RGBA8_UPLOAD,
        }),
        FILL_RECT_WORKLIST_RGBA8_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: FILL_RECT_WORKLIST_RGBA8_ADLS_ARTIFACT,
            gpu: FILL_RECT_WORKLIST_RGBA8_ADLS_GPU,
            upload: &FILL_RECT_WORKLIST_RGBA8_UPLOAD,
        }),
        GRADIENT_RECT_WORKLIST_RGBA8_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: GRADIENT_RECT_WORKLIST_RGBA8_ADLS_ARTIFACT,
            gpu: GRADIENT_RECT_WORKLIST_RGBA8_ADLS_GPU,
            upload: &GRADIENT_RECT_WORKLIST_RGBA8_UPLOAD,
        }),
        ALPHA_BLEND_WORKLIST_RGBA8_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: ALPHA_BLEND_WORKLIST_RGBA8_ADLS_ARTIFACT,
            gpu: ALPHA_BLEND_WORKLIST_RGBA8_ADLS_GPU,
            upload: &ALPHA_BLEND_WORKLIST_RGBA8_UPLOAD,
        }),
        GLYPH_MASK_RGBA8_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: GLYPH_MASK_RGBA8_ADLS_ARTIFACT,
            gpu: GLYPH_MASK_RGBA8_ADLS_GPU,
            upload: &GLYPH_MASK_RGBA8_UPLOAD,
        }),
        FONT_INSTANCE_RGBA8_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: FONT_INSTANCE_RGBA8_ADLS_ARTIFACT,
            gpu: FONT_INSTANCE_RGBA8_ADLS_GPU,
            upload: &FONT_INSTANCE_RGBA8_UPLOAD,
        }),
        HELIO_RETAINED_TRANSFORM_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: HELIO_RETAINED_TRANSFORM_ADLS_ARTIFACT,
            gpu: HELIO_RETAINED_TRANSFORM_ADLS_GPU,
            upload: &HELIO_RETAINED_TRANSFORM_UPLOAD,
        }),
        LAB256_MULTIPHASE_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: LAB256_MULTIPHASE_ADLS_ARTIFACT,
            gpu: LAB256_MULTIPHASE_ADLS_GPU,
            upload: &LAB256_MULTIPHASE_UPLOAD,
        }),
        SPIRIT_VFX_BACKGROUND_RGBA8_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: SPIRIT_VFX_BACKGROUND_RGBA8_ADLS_ARTIFACT,
            gpu: SPIRIT_VFX_BACKGROUND_RGBA8_ADLS_GPU,
            upload: &SPIRIT_VFX_BACKGROUND_RGBA8_UPLOAD,
        }),
        SPIRIT_VFX_SPRITE_RGBA8_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: SPIRIT_VFX_SPRITE_RGBA8_ADLS_ARTIFACT,
            gpu: SPIRIT_VFX_SPRITE_RGBA8_ADLS_GPU,
            upload: &SPIRIT_VFX_SPRITE_RGBA8_UPLOAD,
        }),
        SPRITE_QUAD_WORKLIST_RGBA8_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: SPRITE_QUAD_WORKLIST_RGBA8_ADLS_ARTIFACT,
            gpu: SPRITE_QUAD_WORKLIST_RGBA8_ADLS_GPU,
            upload: &SPRITE_QUAD_WORKLIST_RGBA8_UPLOAD,
        }),
        UI4_COMPOSE_LAYERS_RGBA8_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: UI4_COMPOSE_LAYERS_RGBA8_ADLS_ARTIFACT,
            gpu: UI4_COMPOSE_LAYERS_RGBA8_ADLS_GPU,
            upload: &UI4_COMPOSE_LAYERS_RGBA8_UPLOAD,
        }),
        MANDEL64_WORKLIST_RGBA8_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: MANDEL64_WORKLIST_RGBA8_ADLS_ARTIFACT,
            gpu: MANDEL64_WORKLIST_RGBA8_ADLS_GPU,
            upload: &MANDEL64_WORKLIST_RGBA8_UPLOAD,
        }),
        SKYBOX_SAMPLE_RGB565_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: SKYBOX_SAMPLE_RGB565_ADLS_ARTIFACT,
            gpu: SKYBOX_SAMPLE_RGB565_ADLS_GPU,
            upload: &SKYBOX_SAMPLE_RGB565_UPLOAD,
        }),
        CHART_SINE_RGBA8_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: CHART_SINE_RGBA8_ADLS_ARTIFACT,
            gpu: CHART_SINE_RGBA8_ADLS_GPU,
            upload: &CHART_SINE_RGBA8_UPLOAD,
        }),
        PIXEL_PLASMA_RGBA8_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: PIXEL_PLASMA_RGBA8_ADLS_ARTIFACT,
            gpu: PIXEL_PLASMA_RGBA8_ADLS_GPU,
            upload: &PIXEL_PLASMA_RGBA8_UPLOAD,
        }),
        CPP_DEMO_RGBA8_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: CPP_DEMO_RGBA8_ADLS_ARTIFACT,
            gpu: CPP_DEMO_RGBA8_ADLS_GPU,
            upload: &CPP_DEMO_RGBA8_UPLOAD,
        }),
        SHADERTOY_MANDELBROT_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: SHADERTOY_MANDELBROT_ADLS_ARTIFACT,
            gpu: SHADERTOY_MANDELBROT_ADLS_GPU,
            upload: &SHADERTOY_MANDELBROT_UPLOAD,
        }),
        SHADERTOY_CUBE_FIELD_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: SHADERTOY_CUBE_FIELD_ADLS_ARTIFACT,
            gpu: SHADERTOY_CUBE_FIELD_ADLS_GPU,
            upload: &SHADERTOY_CUBE_FIELD_UPLOAD,
        }),
        SHADERTOY_NGUYEN_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: SHADERTOY_NGUYEN_ADLS_ARTIFACT,
            gpu: SHADERTOY_NGUYEN_ADLS_GPU,
            upload: &SHADERTOY_NGUYEN_UPLOAD,
        }),
        CPP_AUDIO_VISUALIZER_RGBA8_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: CPP_AUDIO_VISUALIZER_RGBA8_ADLS_ARTIFACT,
            gpu: CPP_AUDIO_VISUALIZER_RGBA8_ADLS_GPU,
            upload: &CPP_AUDIO_VISUALIZER_RGBA8_UPLOAD,
        }),
        PARTICLE_CRAFT_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: PARTICLE_CRAFT_ADLS_ARTIFACT,
            gpu: PARTICLE_CRAFT_ADLS_GPU,
            upload: &PARTICLE_CRAFT_UPLOAD,
        }),
        LFM25_Q8_PROJECT_PACKED_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: LFM25_Q8_PROJECT_PACKED_ADLS_ARTIFACT,
            gpu: LFM25_Q8_PROJECT_PACKED_ADLS_GPU,
            upload: &LFM25_Q8_PROJECT_PACKED_UPLOAD,
        }),
        KOKORO_QGEMM_U8_I8_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: KOKORO_QGEMM_U8_I8_ADLS_ARTIFACT,
            gpu: KOKORO_QGEMM_U8_I8_ADLS_GPU,
            upload: &KOKORO_QGEMM_U8_I8_UPLOAD,
        }),
        KOKORO_CONV1D_U8_U8_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: KOKORO_CONV1D_U8_U8_ADLS_ARTIFACT,
            gpu: KOKORO_CONV1D_U8_U8_ADLS_GPU,
            upload: &KOKORO_CONV1D_U8_U8_UPLOAD,
        }),
        SUBSET_SUM_COLLAPSE5_MERGE10_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: SUBSET_SUM_COLLAPSE5_MERGE10_ADLS_ARTIFACT,
            gpu: SUBSET_SUM_COLLAPSE5_MERGE10_ADLS_GPU,
            upload: &SUBSET_SUM_COLLAPSE5_MERGE10_UPLOAD,
        }),
        FONT_OUTLINE_COVERAGE_R8_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: FONT_OUTLINE_COVERAGE_R8_ADLS_ARTIFACT,
            gpu: FONT_OUTLINE_COVERAGE_R8_ADLS_GPU,
            upload: &FONT_OUTLINE_COVERAGE_R8_UPLOAD,
        }),
        _ => None,
    }
}
