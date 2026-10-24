pub(crate) fn copy_rect_rgba8_upload_status() -> Option<UploadedKernelArtifact> {
    *COPY_RECT_RGBA8_UPLOAD.lock()
}



pub(crate) fn alpha_blend_worklist_rgba8_upload_status() -> Option<UploadedKernelArtifact> {
    *ALPHA_BLEND_WORKLIST_RGBA8_UPLOAD.lock()
}


pub(crate) fn sprite_quad_worklist_rgba8_upload_status() -> Option<UploadedKernelArtifact> {
    *SPRITE_QUAD_WORKLIST_RGBA8_UPLOAD.lock()
}

pub(crate) fn ui4_compose_layers_rgba8_upload_status() -> Option<UploadedKernelArtifact> {
    *UI4_COMPOSE_LAYERS_RGBA8_UPLOAD.lock()
}

pub(crate) fn skybox_sample_rgb565_upload_status() -> Option<UploadedKernelArtifact> {
    *SKYBOX_SAMPLE_RGB565_UPLOAD.lock()
}

pub(crate) fn cpp_demo_rgba8_upload_status() -> Option<UploadedKernelArtifact> {
    *CPP_DEMO_RGBA8_UPLOAD.lock()
}

pub(crate) fn cpp_audio_visualizer_rgba8_upload_status() -> Option<UploadedKernelArtifact> {
    *CPP_AUDIO_VISUALIZER_RGBA8_UPLOAD.lock()
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

pub(crate) fn upload_skybox_sample_rgb565_kernel() -> Option<UploadedKernelArtifact> {
    *SKYBOX_SAMPLE_RGB565_UPLOAD.lock()
}

pub(crate) fn register_skybox_package(package: &[u8]) -> bool {
    let Some(contract) = shadertoy_package::contract(17) else {
        return false;
    };
    let Some((bin, spv)) = contract.payloads(package) else {
        contract.report_mismatch(package.len(), "sha256-or-layout");
        return false;
    };
    let Some(dev) = super::claimed_device() else {
        return false;
    };
    let Some(upload) = upload_artifact_bytes(
        dev,
        SKYBOX_SAMPLE_RGB565_ADLS_ARTIFACT,
        SKYBOX_SAMPLE_RGB565_ADLS_GPU,
        bin,
        spv,
        "blueprint",
        "Blueprint:skybox/assets/skybox_sample_rgb565/kernel.clcpp",
        spv.len(),
        GpgpuArtifactAddressSpace::CallerPpgtt,
        *SKYBOX_SAMPLE_RGB565_UPLOAD.lock(),
    ) else {
        return false;
    };
    *SKYBOX_SAMPLE_RGB565_UPLOAD.lock() = Some(upload);
    true
}

/// Executable bytes must first be admitted from a Blueprint-owned package.
pub(crate) fn upload_cpp_demo_rgba8_kernel() -> Option<UploadedKernelArtifact> {
    *CPP_DEMO_RGBA8_UPLOAD.lock()
}

fn shadertoy_artifact_slot(
    shader_id: u32,
) -> Option<(GpgpuKernelArtifact, u64, &'static Mutex<Option<UploadedKernelArtifact>>)> {
    Some(match shader_id {
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
        SHADERTOY_SHADER_MANDELBOX => (
            SHADERTOY_MANDELBOX_ADLS_ARTIFACT,
            SHADERTOY_MANDELBOX_ADLS_GPU,
            &SHADERTOY_MANDELBOX_UPLOAD,
        ),
        SHADERTOY_SHADER_PALETTE_GRID => (
            SHADERTOY_PALETTE_GRID_ADLS_ARTIFACT,
            SHADERTOY_PALETTE_GRID_ADLS_GPU,
            &SHADERTOY_PALETTE_GRID_UPLOAD,
        ),
        SHADERTOY_SHADER_COSMIC_STRANDS => (
            SHADERTOY_COSMIC_STRANDS_ADLS_ARTIFACT,
            SHADERTOY_COSMIC_STRANDS_ADLS_GPU,
            &SHADERTOY_COSMIC_STRANDS_UPLOAD,
        ),
        SHADERTOY_SHADER_PROTEAN_CLOUDS => (
            SHADERTOY_PROTEAN_CLOUDS_ADLS_ARTIFACT,
            SHADERTOY_PROTEAN_CLOUDS_ADLS_GPU,
            &SHADERTOY_PROTEAN_CLOUDS_UPLOAD,
        ),
        7 => (
            CPP_AUDIO_VISUALIZER_RGBA8_ADLS_ARTIFACT,
            CPP_AUDIO_VISUALIZER_RGBA8_ADLS_GPU,
            &CPP_AUDIO_VISUALIZER_RGBA8_UPLOAD,
        ),
        8 => (CPP_DEMO_RGBA8_ADLS_ARTIFACT, CPP_DEMO_RGBA8_ADLS_GPU, &CPP_DEMO_RGBA8_UPLOAD),
        15 => (PARTICLE_CRAFT_ADLS_ARTIFACT, PARTICLE_CRAFT_ADLS_GPU, &PARTICLE_CRAFT_UPLOAD),
        _ => return None,
    })
}

/// Dispatch only already-authenticated resident code. No filesystem/embedded fallback.
pub(crate) fn upload_shadertoy_kernel(shader_id: u32) -> Option<UploadedKernelArtifact> {
    let (_, _, slot) = shadertoy_artifact_slot(shader_id)?;
    *slot.lock()
}

/// `package` is a complete kernel-owned copy, never a borrowed guest buffer.
pub(crate) fn register_shadertoy_package(shader_id: u32, package: &[u8]) -> bool {
    let Some(contract) = shadertoy_package::contract(shader_id) else {
        return false;
    };
    let Some((bin, spv)) = contract.payloads(package) else {
        contract.report_mismatch(package.len(), "sha256-or-layout");
        return false;
    };
    let Some((artifact, gpu, slot)) = shadertoy_artifact_slot(shader_id) else {
        return false;
    };
    let Some(dev) = super::claimed_device() else {
        return false;
    };
    // Serialize first upload and reuse; resident immutable code is never replaced.
    if shader_id == 15 {
        // ParticleCraft has three entry points in one ELF. Its legacy upload
        // descriptor has no single ABI; check every generated entry explicitly.
        for abi in [
            &PARTICLE_CRAFT_STEP_ADLS_CPP_ABI_CONTRACT,
            &PARTICLE_CRAFT_BIN_TILES_ADLS_CPP_ABI_CONTRACT,
            &PARTICLE_CRAFT_RENDER_RGBA8_ADLS_CPP_ABI_CONTRACT,
        ] {
            if admit_kernel_artifact_payloads(
                GpgpuKernelArtifact::contracted(abi.kernel_name, &[], &[], abi),
                dev.device_id,
                dev.revision_id,
                bin,
                spv,
            )
            .is_err()
            {
                return false;
            }
        }
    }
    let mut resident = slot.lock();
    let Some(upload) = upload_artifact_bytes(
        dev,
        artifact,
        gpu,
        bin,
        spv,
        "blueprint",
        artifact.name,
        spv.len(),
        GpgpuArtifactAddressSpace::CallerPpgtt,
        *resident,
    ) else {
        return false;
    };
    *resident = Some(upload);
    true
}

/// Executable bytes must first be admitted from a Blueprint-owned package.
pub(crate) fn upload_cpp_audio_visualizer_rgba8_kernel() -> Option<UploadedKernelArtifact> {
    *CPP_AUDIO_VISUALIZER_RGBA8_UPLOAD.lock()
}

/// Executable bytes must first be admitted from a Blueprint-owned package.
pub(crate) fn upload_particle_craft_kernel() -> Option<UploadedKernelArtifact> {
    *PARTICLE_CRAFT_UPLOAD.lock()
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
    ALPHA_BLEND_WORKLIST_RGBA8_KERNEL_NAME,
    SPRITE_QUAD_WORKLIST_RGBA8_KERNEL_NAME,
    UI4_COMPOSE_LAYERS_RGBA8_KERNEL_NAME,
    SKYBOX_SAMPLE_RGB565_KERNEL_NAME,
    CPP_DEMO_RGBA8_KERNEL_NAME,
    SHADERTOY_MANDELBROT_KERNEL_NAME,
    SHADERTOY_CUBE_FIELD_KERNEL_NAME,
    SHADERTOY_NGUYEN_KERNEL_NAME,
    SHADERTOY_MANDELBOX_KERNEL_NAME,
    SHADERTOY_PALETTE_GRID_KERNEL_NAME,
    SHADERTOY_COSMIC_STRANDS_KERNEL_NAME,
    PARTICLE_CRAFT_KERNEL_NAME,
    HELIO_RETAINED_TRANSFORM_KERNEL_NAME,
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
        | ALPHA_BLEND_WORKLIST_RGBA8_KERNEL_NAME
        | UI4_NV12_TILE64_TO_RGBA8_FRAME_KERNEL_NAME
        | SPRITE_QUAD_WORKLIST_RGBA8_KERNEL_NAME
        | UI4_COMPOSE_LAYERS_RGBA8_KERNEL_NAME
        | SKYBOX_SAMPLE_RGB565_KERNEL_NAME
        | CPP_DEMO_RGBA8_KERNEL_NAME
        | SHADERTOY_MANDELBROT_KERNEL_NAME
        | SHADERTOY_CUBE_FIELD_KERNEL_NAME
        | SHADERTOY_NGUYEN_KERNEL_NAME
        | SHADERTOY_MANDELBOX_KERNEL_NAME
        | SHADERTOY_PALETTE_GRID_KERNEL_NAME
        | SHADERTOY_COSMIC_STRANDS_KERNEL_NAME
        | CPP_AUDIO_VISUALIZER_RGBA8_KERNEL_NAME
        | PARTICLE_CRAFT_KERNEL_NAME
        | HELIO_RETAINED_TRANSFORM_KERNEL_NAME
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

        ALPHA_BLEND_WORKLIST_RGBA8_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: ALPHA_BLEND_WORKLIST_RGBA8_ADLS_ARTIFACT,
            gpu: ALPHA_BLEND_WORKLIST_RGBA8_ADLS_GPU,
            upload: &ALPHA_BLEND_WORKLIST_RGBA8_UPLOAD,
        }),


        HELIO_RETAINED_TRANSFORM_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: HELIO_RETAINED_TRANSFORM_ADLS_ARTIFACT,
            gpu: HELIO_RETAINED_TRANSFORM_ADLS_GPU,
            upload: &HELIO_RETAINED_TRANSFORM_UPLOAD,
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
        SKYBOX_SAMPLE_RGB565_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: SKYBOX_SAMPLE_RGB565_ADLS_ARTIFACT,
            gpu: SKYBOX_SAMPLE_RGB565_ADLS_GPU,
            upload: &SKYBOX_SAMPLE_RGB565_UPLOAD,
        }),
        CPP_DEMO_RGBA8_KERNEL_NAME => Some(GpgpuKnownArtifactSlot {
            artifact: CPP_DEMO_RGBA8_ADLS_ARTIFACT,
            gpu: CPP_DEMO_RGBA8_ADLS_GPU,
            upload: &CPP_DEMO_RGBA8_UPLOAD,
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





        _ => None,
    }
}
