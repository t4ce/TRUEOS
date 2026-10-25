#[derive(Copy, Clone, Debug)]
pub(crate) struct GpgpuKernelArtifact {
    pub(crate) name: &'static str,
    pub(crate) target: &'static str,
    pub(crate) target_policy: GpgpuKernelTarget,
    pub(crate) bin: &'static [u8],
    pub(crate) spv: &'static [u8],
    pub(crate) bin_sha256: [u8; 32],
    pub(crate) abi_contract: Option<&'static GpgpuKernelAbiContract>,
}

impl GpgpuKernelArtifact {
    pub(crate) const fn new(
        name: &'static str,
        target_policy: GpgpuKernelTarget,
        bin: &'static [u8],
        spv: &'static [u8],
        bin_sha256: [u8; 32],
        abi_contract: Option<&'static GpgpuKernelAbiContract>,
    ) -> Self {
        Self {
            name,
            target: target_policy.label,
            target_policy,
            bin,
            spv,
            bin_sha256,
            abi_contract,
        }
    }

    const fn contracted(
        name: &'static str,
        bin: &'static [u8],
        spv: &'static [u8],
        contract: &'static GpgpuKernelAbiContract,
    ) -> Self {
        Self::new(name, contract.target, bin, spv, contract.zebin_sha256, Some(contract))
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct UploadedKernelArtifact {
    pub(crate) name: &'static str,
    pub(crate) target: &'static str,
    pub(crate) source: &'static str,
    pub(crate) gpu: u64,
    pub(crate) phys: u64,
    pub(crate) bytes: usize,
    pub(crate) mapped_bytes: usize,
    pub(crate) verified: bool,
    pub(crate) bin_sha256: [u8; 32],
    pub(crate) device_id: u16,
    pub(crate) revision_id: u8,
    pub(crate) abi_schema_version: Option<u16>,
    pub(crate) address_space: GpgpuArtifactAddressSpace,
}

unsafe impl Send for UploadedKernelArtifact {}
unsafe impl Sync for UploadedKernelArtifact {}

/// Address-space ownership established by the artifact upload.
///
/// `CallerPpgtt` deliberately means the upload owns authenticated, flushed
/// DMA pages but has not installed a global GGTT alias. The consuming context
/// must map those pages into its own PPGTT before encoding the instruction VA.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum GpgpuArtifactAddressSpace {
    GlobalGgtt,
    CallerPpgtt,
}

impl GpgpuArtifactAddressSpace {
    const fn label(self) -> &'static str {
        match self {
            Self::GlobalGgtt => "global-ggtt",
            Self::CallerPpgtt => "caller-ppgtt",
        }
    }
}

pub(crate) const COPY_RECT_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact::contracted(
        COPY_RECT_RGBA8_KERNEL_NAME,
        COPY_RECT_RGBA8_ADLS_BIN,
        COPY_RECT_RGBA8_ADLS_SPV,
        &COPY_RECT_RGBA8_ADLS_CPP_ABI_CONTRACT,
    );



pub(crate) const ALPHA_BLEND_WORKLIST_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact::contracted(
        ALPHA_BLEND_WORKLIST_RGBA8_KERNEL_NAME,
        ALPHA_BLEND_WORKLIST_RGBA8_ADLS_BIN,
        ALPHA_BLEND_WORKLIST_RGBA8_ADLS_SPV,
        &ALPHA_BLEND_WORKLIST_RGBA8_ADLS_CPP_ABI_CONTRACT,
    );


pub(crate) const UI4_NV12_TILE64_TO_RGBA8_FRAME_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact::contracted(
        UI4_NV12_TILE64_TO_RGBA8_FRAME_KERNEL_NAME,
        UI4_NV12_TILE64_TO_RGBA8_FRAME_ADLS_BIN,
        UI4_NV12_TILE64_TO_RGBA8_FRAME_ADLS_SPV,
        &UI4_NV12_TILE64_TO_RGBA8_FRAME_ADLS_CPP_ABI_CONTRACT,
    );

pub(crate) const SPRITE_QUAD_WORKLIST_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact::contracted(
        SPRITE_QUAD_WORKLIST_RGBA8_KERNEL_NAME,
        SPRITE_QUAD_WORKLIST_RGBA8_ADLS_BIN,
        SPRITE_QUAD_WORKLIST_RGBA8_ADLS_SPV,
        &SPRITE_QUAD_WORKLIST_RGBA8_ADLS_CPP_ABI_CONTRACT,
    );

pub(crate) const UI4_COMPOSE_LAYERS_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact::contracted(
        UI4_COMPOSE_LAYERS_RGBA8_KERNEL_NAME,
        UI4_COMPOSE_LAYERS_RGBA8_ADLS_BIN,
        UI4_COMPOSE_LAYERS_RGBA8_ADLS_SPV,
        &UI4_COMPOSE_LAYERS_RGBA8_ADLS_CPP_ABI_CONTRACT,
    );

pub(crate) const SKYBOX_SAMPLE_RGB565_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact::contracted(
        SKYBOX_SAMPLE_RGB565_KERNEL_NAME,
        &[],
        &[],
        &SKYBOX_SAMPLE_RGB565_ADLS_CPP_ABI_CONTRACT,
    );

pub(crate) const CPP_DEMO_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact = GpgpuKernelArtifact::new(
    CPP_DEMO_RGBA8_KERNEL_NAME,
    CPP_DEMO_RGBA8_ADLS_CPP_ABI_CONTRACT.target,
    &[],
    &[],
    CPP_DEMO_RGBA8_ADLS_BIN_SHA256,
    Some(&CPP_DEMO_RGBA8_ADLS_CPP_ABI_CONTRACT),
);

pub(crate) const SHADERTOY_MANDELBROT_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact::contracted(
        SHADERTOY_MANDELBROT_KERNEL_NAME,
        &[], // Payload supplied by the Blueprint after package authentication.
        &[], // Payload supplied by the Blueprint after package authentication.
        &SHADERTOY_MANDELBROT_ADLS_CPP_ABI_CONTRACT,
    );

pub(crate) const SHADERTOY_CUBE_FIELD_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact::contracted(
        SHADERTOY_CUBE_FIELD_KERNEL_NAME,
        &[], // Payload supplied by the Blueprint after package authentication.
        &[], // Payload supplied by the Blueprint after package authentication.
        &SHADERTOY_CUBE_FIELD_ADLS_CPP_ABI_CONTRACT,
    );

pub(crate) const SHADERTOY_NGUYEN_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact::contracted(
        SHADERTOY_NGUYEN_KERNEL_NAME,
        &[], // Payload supplied by the Blueprint after package authentication.
        &[], // Payload supplied by the Blueprint after package authentication.
        &SHADERTOY_NGUYEN_ADLS_CPP_ABI_CONTRACT,
    );

pub(crate) const SHADERTOY_MANDELBOX_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact::contracted(
        SHADERTOY_MANDELBOX_KERNEL_NAME,
        &[], // Payload supplied by the Blueprint after package authentication.
        &[], // Payload supplied by the Blueprint after package authentication.
        &SHADERTOY_MANDELBOX_ADLS_CPP_ABI_CONTRACT,
    );

pub(crate) const SHADERTOY_PALETTE_GRID_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact::contracted(
        SHADERTOY_PALETTE_GRID_KERNEL_NAME,
        &[], // Payload supplied by the Blueprint after package authentication.
        &[], // Payload supplied by the Blueprint after package authentication.
        &SHADERTOY_PALETTE_GRID_ADLS_CPP_ABI_CONTRACT,
    );

pub(crate) const SHADERTOY_COSMIC_STRANDS_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact::contracted(
        SHADERTOY_COSMIC_STRANDS_KERNEL_NAME,
        &[], // Payload supplied by the Blueprint after package authentication.
        &[], // Payload supplied by the Blueprint after package authentication.
        &SHADERTOY_COSMIC_STRANDS_ADLS_CPP_ABI_CONTRACT,
    );

pub(crate) const SHADERTOY_PROTEAN_CLOUDS_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact::contracted(
        SHADERTOY_PROTEAN_CLOUDS_KERNEL_NAME,
        &[], // Blueprint package supplies the authenticated binary and SPIR-V.
        &[],
        &SHADERTOY_PROTEAN_CLOUDS_ADLS_CPP_ABI_CONTRACT,
    );

pub(crate) const CPP_AUDIO_VISUALIZER_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact::new(
        CPP_AUDIO_VISUALIZER_RGBA8_KERNEL_NAME,
        CPP_AUDIO_VISUALIZER_RGBA8_ADLS_CPP_ABI_CONTRACT.target,
        &[],
        &[],
        CPP_AUDIO_VISUALIZER_RGBA8_ADLS_BIN_SHA256,
        Some(&CPP_AUDIO_VISUALIZER_RGBA8_ADLS_CPP_ABI_CONTRACT),
    );

pub(crate) const PARTICLE_CRAFT_ADLS_ARTIFACT: GpgpuKernelArtifact = GpgpuKernelArtifact::new(
    PARTICLE_CRAFT_KERNEL_NAME,
    PARTICLE_CRAFT_STEP_ADLS_CPP_ABI_CONTRACT.target,
    &[],
    &[],
    PARTICLE_CRAFT_ADLS_BIN_SHA256,
    None,
);




pub(crate) const HELIO_RETAINED_TRANSFORM_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact::contracted(
        HELIO_RETAINED_TRANSFORM_KERNEL_NAME,
        HELIO_RETAINED_TRANSFORM_ADLS_BIN,
        HELIO_RETAINED_TRANSFORM_ADLS_SPV,
        &HELIO_RETAINED_TRANSFORM_ADLS_CPP_ABI_CONTRACT,
    );


pub(crate) const SPIRIT_VFX_BACKGROUND_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact::new(
        SPIRIT_VFX_BACKGROUND_RGBA8_KERNEL_NAME,
        SPIRIT_VFX_BACKGROUND_RGBA8_ADLS_CPP_ABI_CONTRACT.target,
        SPIRIT_VFX_BACKGROUND_RGBA8_ADLS_BIN,
        SPIRIT_VFX_BACKGROUND_RGBA8_ADLS_SPV,
        SPIRIT_VFX_BACKGROUND_RGBA8_ADLS_BIN_SHA256,
        Some(&SPIRIT_VFX_BACKGROUND_RGBA8_ADLS_CPP_ABI_CONTRACT),
    );

pub(crate) const SPIRIT_VFX_SPRITE_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact::new(
        SPIRIT_VFX_SPRITE_RGBA8_KERNEL_NAME,
        SPIRIT_VFX_SPRITE_RGBA8_ADLS_CPP_ABI_CONTRACT.target,
        SPIRIT_VFX_SPRITE_RGBA8_ADLS_BIN,
        SPIRIT_VFX_SPRITE_RGBA8_ADLS_SPV,
        SPIRIT_VFX_SPRITE_RGBA8_ADLS_BIN_SHA256,
        Some(&SPIRIT_VFX_SPRITE_RGBA8_ADLS_CPP_ABI_CONTRACT),
    );
