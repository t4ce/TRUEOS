pub(crate) fn copy_rect_rgba8_upload_status() -> Option<UploadedKernelArtifact> {
    *COPY_RECT_RGBA8_UPLOAD.lock()
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

pub(crate) fn font_outline_mesh_upload_status() -> Option<UploadedKernelArtifact> {
    *FONT_OUTLINE_MESH_UPLOAD.lock()
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

pub(crate) fn upload_resolve_tile64_msaa4_rgba8_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *RESOLVE_TILE64_MSAA4_RGBA8_UPLOAD.lock() {
        return Some(upload);
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: resolve-tile64-msaa4-rgba8 upload skipped reason=no-claimed-device\n"
        );
        return None;
    };

    let upload = upload_artifact(
        dev,
        RESOLVE_TILE64_MSAA4_RGBA8_ADLS_ARTIFACT,
        RESOLVE_TILE64_MSAA4_RGBA8_ADLS_GPU,
    )?;
    *RESOLVE_TILE64_MSAA4_RGBA8_UPLOAD.lock() = Some(upload);
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

pub(crate) fn upload_ui4_nv12_ytile_to_primary_xrgb_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *UI4_NV12_YTILE_TO_PRIMARY_XRGB_UPLOAD.lock() {
        return Some(upload);
    }
    let dev = super::claimed_device()?;
    let upload = upload_artifact(
        dev,
        UI4_NV12_YTILE_TO_PRIMARY_XRGB_ADLS_ARTIFACT,
        UI4_NV12_YTILE_TO_PRIMARY_XRGB_ADLS_GPU,
    )?;
    *UI4_NV12_YTILE_TO_PRIMARY_XRGB_UPLOAD.lock() = Some(upload);
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

pub(crate) fn upload_font_outline_mesh_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *FONT_OUTLINE_MESH_UPLOAD.lock() {
        return Some(upload);
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_warn!(
            target: "gpgpu";
            "intel/gpgpu: font-outline-mesh upload skipped reason=no-claimed-device\n"
        );
        return None;
    };

    let upload = upload_artifact(dev, FONT_OUTLINE_MESH_ADLS_ARTIFACT, FONT_OUTLINE_MESH_ADLS_GPU)?;
    *FONT_OUTLINE_MESH_UPLOAD.lock() = Some(upload);
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

pub(crate) fn upload_scene_aabb_kernel() -> Option<UploadedKernelArtifact> {
    if let Some(upload) = *SCENE_AABB_UPLOAD.lock() {
        return Some(upload);
    }
    let dev = super::claimed_device()?;
    let upload = upload_artifact(dev, SCENE_AABB_ADLS_ARTIFACT, SCENE_AABB_ADLS_GPU)?;
    *SCENE_AABB_UPLOAD.lock() = Some(upload);
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
    RESOLVE_TILE64_MSAA4_RGBA8_KERNEL_NAME,
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
    FONT_OUTLINE_MESH_KERNEL_NAME,
    FONT_OUTLINE_COVERAGE_R8_KERNEL_NAME,
    SCENE_AABB_KERNEL_NAME,
];

pub(crate) fn reload_known_kernel_artifact(
    name: &str,
) -> Result<UploadedKernelArtifact, GpgpuArtifactReloadError> {
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

    let Some(upload) = upload_artifact_from_sources(dev, slot.artifact, slot.gpu, true) else {
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
        RESOLVE_TILE64_MSAA4_RGBA8_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: RESOLVE_TILE64_MSAA4_RGBA8_ADLS_ARTIFACT,
            gpu: RESOLVE_TILE64_MSAA4_RGBA8_ADLS_GPU,
            upload: &RESOLVE_TILE64_MSAA4_RGBA8_UPLOAD,
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
        SCENE_AABB_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: SCENE_AABB_ADLS_ARTIFACT,
            gpu: SCENE_AABB_ADLS_GPU,
            upload: &SCENE_AABB_UPLOAD,
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
        FONT_OUTLINE_MESH_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: FONT_OUTLINE_MESH_ADLS_ARTIFACT,
            gpu: FONT_OUTLINE_MESH_ADLS_GPU,
            upload: &FONT_OUTLINE_MESH_UPLOAD,
        }),
        FONT_OUTLINE_COVERAGE_R8_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: FONT_OUTLINE_COVERAGE_R8_ADLS_ARTIFACT,
            gpu: FONT_OUTLINE_COVERAGE_R8_ADLS_GPU,
            upload: &FONT_OUTLINE_COVERAGE_R8_UPLOAD,
        }),
        _ => None,
    }
}
