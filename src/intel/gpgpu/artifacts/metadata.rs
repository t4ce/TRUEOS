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

    const fn multi_entry(
        name: &'static str,
        bin: &'static [u8],
        spv: &'static [u8],
        first_entry: &'static GpgpuKernelAbiContract,
    ) -> Self {
        Self::new(name, first_entry.target, bin, spv, first_entry.zebin_sha256, None)
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
}

unsafe impl Send for UploadedKernelArtifact {}
unsafe impl Sync for UploadedKernelArtifact {}

pub(crate) const COPY_RECT_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact::contracted(
        COPY_RECT_RGBA8_KERNEL_NAME,
        COPY_RECT_RGBA8_ADLS_BIN,
        COPY_RECT_RGBA8_ADLS_SPV,
        &COPY_RECT_RGBA8_ADLS_CPP_ABI_CONTRACT,
    );

pub(crate) const RESOLVE_TILE64_MSAA4_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact::contracted(
        RESOLVE_TILE64_MSAA4_RGBA8_KERNEL_NAME,
        RESOLVE_TILE64_MSAA4_RGBA8_ADLS_BIN,
        RESOLVE_TILE64_MSAA4_RGBA8_ADLS_SPV,
        &RESOLVE_TILE64_MSAA4_RGBA8_ADLS_CPP_ABI_CONTRACT,
    );

pub(crate) const FILL_RECT_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact::contracted(
        FILL_RECT_RGBA8_KERNEL_NAME,
        FILL_RECT_RGBA8_ADLS_BIN,
        FILL_RECT_RGBA8_ADLS_SPV,
        &FILL_RECT_RGBA8_ADLS_CPP_ABI_CONTRACT,
    );

pub(crate) const FILL_RECT_WORKLIST_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact::contracted(
        FILL_RECT_WORKLIST_RGBA8_KERNEL_NAME,
        FILL_RECT_WORKLIST_RGBA8_ADLS_BIN,
        FILL_RECT_WORKLIST_RGBA8_ADLS_SPV,
        &FILL_RECT_WORKLIST_RGBA8_ADLS_CPP_ABI_CONTRACT,
    );

pub(crate) const GRADIENT_RECT_WORKLIST_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact::contracted(
        GRADIENT_RECT_WORKLIST_RGBA8_KERNEL_NAME,
        GRADIENT_RECT_WORKLIST_RGBA8_ADLS_BIN,
        GRADIENT_RECT_WORKLIST_RGBA8_ADLS_SPV,
        &GRADIENT_RECT_WORKLIST_RGBA8_ADLS_CPP_ABI_CONTRACT,
    );

pub(crate) const ALPHA_BLEND_WORKLIST_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact::contracted(
        ALPHA_BLEND_WORKLIST_RGBA8_KERNEL_NAME,
        ALPHA_BLEND_WORKLIST_RGBA8_ADLS_BIN,
        ALPHA_BLEND_WORKLIST_RGBA8_ADLS_SPV,
        &ALPHA_BLEND_WORKLIST_RGBA8_ADLS_CPP_ABI_CONTRACT,
    );

pub(crate) const GLYPH_MASK_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact::contracted(
        GLYPH_MASK_RGBA8_KERNEL_NAME,
        GLYPH_MASK_RGBA8_ADLS_BIN,
        GLYPH_MASK_RGBA8_ADLS_SPV,
        &GLYPH_MASK_RGBA8_ADLS_CPP_ABI_CONTRACT,
    );

pub(crate) const UI4_NV12_TILE64_TO_RGBA8_FRAME_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact::contracted(
        UI4_NV12_TILE64_TO_RGBA8_FRAME_KERNEL_NAME,
        UI4_NV12_TILE64_TO_RGBA8_FRAME_ADLS_BIN,
        UI4_NV12_TILE64_TO_RGBA8_FRAME_ADLS_SPV,
        &UI4_NV12_TILE64_TO_RGBA8_FRAME_ADLS_CPP_ABI_CONTRACT,
    );

pub(crate) const UI4_RGBA8_TO_NV12_LINEAR_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact::contracted(
        UI4_RGBA8_TO_NV12_LINEAR_KERNEL_NAME,
        UI4_RGBA8_TO_NV12_LINEAR_ADLS_BIN,
        UI4_RGBA8_TO_NV12_LINEAR_ADLS_SPV,
        &UI4_RGBA8_TO_NV12_LINEAR_ADLS_CPP_ABI_CONTRACT,
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

pub(crate) const MANDEL64_WORKLIST_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact::contracted(
        MANDEL64_WORKLIST_RGBA8_KERNEL_NAME,
        MANDEL64_WORKLIST_RGBA8_ADLS_BIN,
        MANDEL64_WORKLIST_RGBA8_ADLS_SPV,
        &MANDEL64_WORKLIST_RGBA8_ADLS_CPP_ABI_CONTRACT,
    );

pub(crate) const SKYBOX_SAMPLE_RGB565_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact::contracted(
        SKYBOX_SAMPLE_RGB565_KERNEL_NAME,
        SKYBOX_SAMPLE_RGB565_ADLS_BIN,
        SKYBOX_SAMPLE_RGB565_ADLS_SPV,
        &SKYBOX_SAMPLE_RGB565_ADLS_CPP_ABI_CONTRACT,
    );

pub(crate) const CHART_SINE_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact::contracted(
        CHART_SINE_RGBA8_KERNEL_NAME,
        CHART_SINE_RGBA8_ADLS_BIN,
        CHART_SINE_RGBA8_ADLS_SPV,
        &CHART_SINE_RGBA8_ADLS_CPP_ABI_CONTRACT,
    );

pub(crate) const PIXEL_PLASMA_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact::contracted(
        PIXEL_PLASMA_RGBA8_KERNEL_NAME,
        PIXEL_PLASMA_RGBA8_ADLS_BIN,
        PIXEL_PLASMA_RGBA8_ADLS_SPV,
        &PIXEL_PLASMA_RGBA8_ADLS_CPP_ABI_CONTRACT,
    );

pub(crate) const CPP_DEMO_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact = GpgpuKernelArtifact::new(
    CPP_DEMO_RGBA8_KERNEL_NAME,
    CPP_DEMO_RGBA8_ADLS_CPP_ABI_CONTRACT.target,
    CPP_DEMO_RGBA8_ADLS_BIN,
    CPP_DEMO_RGBA8_ADLS_SPV,
    CPP_DEMO_RGBA8_ADLS_BIN_SHA256,
    Some(&CPP_DEMO_RGBA8_ADLS_CPP_ABI_CONTRACT),
);

pub(crate) const CPP_AUDIO_VISUALIZER_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact::new(
        CPP_AUDIO_VISUALIZER_RGBA8_KERNEL_NAME,
        CPP_AUDIO_VISUALIZER_RGBA8_ADLS_CPP_ABI_CONTRACT.target,
        CPP_AUDIO_VISUALIZER_RGBA8_ADLS_BIN,
        CPP_AUDIO_VISUALIZER_RGBA8_ADLS_SPV,
        CPP_AUDIO_VISUALIZER_RGBA8_ADLS_BIN_SHA256,
        Some(&CPP_AUDIO_VISUALIZER_RGBA8_ADLS_CPP_ABI_CONTRACT),
    );

pub(crate) const PARTICLE_CRAFT_ADLS_ARTIFACT: GpgpuKernelArtifact = GpgpuKernelArtifact::new(
    PARTICLE_CRAFT_KERNEL_NAME,
    PARTICLE_CRAFT_STEP_ADLS_CPP_ABI_CONTRACT.target,
    PARTICLE_CRAFT_ADLS_BIN,
    PARTICLE_CRAFT_ADLS_SPV,
    PARTICLE_CRAFT_ADLS_BIN_SHA256,
    None,
);

pub(crate) const FONT_INSTANCE_RGBA8_ADLS_ARTIFACT: GpgpuKernelArtifact = GpgpuKernelArtifact::new(
    FONT_INSTANCE_RGBA8_KERNEL_NAME,
    FONT_INSTANCE_RGBA8_ADLS_CPP_ABI_CONTRACT.target,
    FONT_INSTANCE_RGBA8_ADLS_BIN,
    FONT_INSTANCE_RGBA8_ADLS_SPV,
    FONT_INSTANCE_RGBA8_ADLS_BIN_SHA256,
    Some(&FONT_INSTANCE_RGBA8_ADLS_CPP_ABI_CONTRACT),
);

pub(crate) const LFM25_Q8_PROJECT_PACKED_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact::new(
        LFM25_Q8_PROJECT_PACKED_KERNEL_NAME,
        LFM25_Q8_PROJECT_PACKED_ADLS_CPP_ABI_CONTRACT.target,
        LFM25_Q8_PROJECT_PACKED_ADLS_BIN,
        LFM25_Q8_PROJECT_PACKED_ADLS_SPV,
        LFM25_Q8_PROJECT_PACKED_ADLS_BIN_SHA256,
        Some(&LFM25_Q8_PROJECT_PACKED_ADLS_CPP_ABI_CONTRACT),
    );

pub(crate) const FONT_OUTLINE_COVERAGE_R8_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact::contracted(
        FONT_OUTLINE_COVERAGE_R8_KERNEL_NAME,
        FONT_OUTLINE_COVERAGE_R8_ADLS_BIN,
        FONT_OUTLINE_COVERAGE_R8_ADLS_SPV,
        &FONT_OUTLINE_COVERAGE_R8_ADLS_CPP_ABI_CONTRACT,
    );

pub(crate) const SCENE_AABB_ADLS_ARTIFACT: GpgpuKernelArtifact = GpgpuKernelArtifact::contracted(
    SCENE_AABB_KERNEL_NAME,
    SCENE_AABB_ADLS_BIN,
    SCENE_AABB_ADLS_SPV,
    &SCENE_AABB_ADLS_CPP_ABI_CONTRACT,
);

pub(crate) const LAB256_MULTIPHASE_ADLS_ARTIFACT: GpgpuKernelArtifact =
    GpgpuKernelArtifact::multi_entry(
        LAB256_MULTIPHASE_KERNEL_NAME,
        LAB256_MULTIPHASE_ADLS_BIN,
        LAB256_MULTIPHASE_ADLS_SPV,
        &LAB256_STEP_ADLS_CPP_ABI_CONTRACT,
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
